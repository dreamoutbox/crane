use crate::{
    config,
    etcd_unit::{install_etcd, setup_etcd, start_etcd, wait_for_etcd_cluster},
    haproxy_unit::haproxy::setup_haproxy_on_each_nodes_wrapper,
    helper::config::config_get_nodes,
    patroni::install_patroni,
    postgres_unit::{
        helper::{
            backup_postgres_dir, configure_postgres_cron_backup, get_pg_version,
            get_postgres_backup_schedule, get_postgres_configs, get_replica_pass,
            pg_clear_dcs_state, pg_cluster_wait_all_nodes_ready, postgres_get_primary,
        },
        install::install_postgres,
        setup_postgres_primary::setup_postgres_primary,
    },
    server_interactor::{get_server_interactor, server_interactor_trait::ServerInteractor},
};

pub async fn postgres_setup_wrapper(
    config: &config::Config,
    app_nodes: &Vec<config::NodeConfig>,
) -> Result<(), anyhow::Error> {
    let pg_version = get_pg_version(&config);
    let replica_pass = get_replica_pass(&config);

    let schedule = get_postgres_backup_schedule(config);
    let s3_config = if schedule.is_some() {
        Some(crate::s3::get_s3_config(config)?)
    } else {
        None
    };

    let pg_nodes = config_get_nodes(&config, "postgres");

    if pg_nodes.is_empty() {
        return Ok(());
    }

    // Phase 1, 2, and 3 coordinated across all postgres nodes concurrently using Barriers
    let num_nodes = pg_nodes.len();
    let barrier_installed = std::sync::Arc::new(std::sync::Barrier::new(num_nodes));
    let barrier_etcd_started = std::sync::Arc::new(std::sync::Barrier::new(num_nodes));
    let barrier_quorum = std::sync::Arc::new(std::sync::Barrier::new(num_nodes));

    let mut handles = vec![];
    for (i, node) in pg_nodes.iter().enumerate() {
        let node = node.clone();
        let pg_nodes = pg_nodes.clone();
        let pg_version = pg_version.clone();
        let replica_pass = replica_pass.clone();
        let schedule = schedule.clone();
        let s3_config = s3_config.clone();

        let barrier_installed = barrier_installed.clone();
        let barrier_etcd_started = barrier_etcd_started.clone();
        let barrier_quorum = barrier_quorum.clone();
        let is_first_node = i == 0;

        let handle = tokio::task::spawn_blocking(move || {
            inner_setup_postgres_node(
                node,
                pg_nodes,
                pg_version,
                replica_pass,
                schedule,
                s3_config,
                barrier_installed,
                barrier_etcd_started,
                barrier_quorum,
                is_first_node,
            )
        });

        handles.push(handle);
    }

    let mut interactors = std::collections::HashMap::new();
    for handle in handles {
        let (node_name, interactor) = handle.await??;
        interactors.insert(node_name, interactor);
    }

    // 2. Wait for Patroni leader election
    println!("\tWaiting for Patroni leader election...");
    let mut primary_node = None;
    for _ in 0..100 {
        if let Ok(Some(leader)) = postgres_get_primary(config) {
            primary_node = Some(leader);
            break;
        }

        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    let primary = primary_node
        .ok_or_else(|| anyhow::anyhow!("Timeout waiting for PostgreSQL Patroni leader election"))?;
    println!("\tDiscovered Patroni leader at node: {}", primary.name);

    let replicas: Vec<String> = pg_nodes
        .iter()
        .filter(|n| n.internal_ip != primary.internal_ip)
        .map(|n| n.name.clone())
        .collect();
    println!("\tReplicas: {:?}", replicas);

    // 3. Provision database schema and users on the dynamic leader
    let leader_interactor = interactors
        .remove(&primary.name)
        .ok_or_else(|| anyhow::anyhow!("Leader interactor not found in setup map"))?;
    let (db_configs, user_configs) = get_postgres_configs(config);

    setup_postgres_primary(
        leader_interactor.clone(),
        &pg_version,
        &replica_pass,
        &db_configs,
        &user_configs,
        config,
    )
    .await?;

    // 4. Assert all patroni instances are healthy
    println!("\n\tPolling PostgreSQL cluster health...");
    let start = std::time::Instant::now();
    pg_cluster_wait_all_nodes_ready(&*leader_interactor, &pg_nodes);

    let elapsed = start.elapsed();
    println!(
        "\tPostgreSQL cluster health check completed (took {}s)",
        elapsed.as_secs()
    );

    // 5. Setup HAProxy on all app nodes
    setup_haproxy_on_each_nodes_wrapper(config, app_nodes).await?;

    Ok(())
}

fn inner_setup_postgres_node(
    node: config::NodeConfig,
    pg_nodes: Vec<config::NodeConfig>,
    pg_version: String,
    replica_pass: String,
    schedule: Option<config::PostgresBackupSchedule>,
    s3_config: Option<crate::s3::S3Config>,
    barrier_installed: std::sync::Arc<std::sync::Barrier>,
    barrier_etcd_started: std::sync::Arc<std::sync::Barrier>,
    barrier_quorum: std::sync::Arc<std::sync::Barrier>,
    is_first_node: bool,
) -> anyhow::Result<(String, std::sync::Arc<dyn ServerInteractor + Send + Sync>)> {
    println!("Configuring node {}...", node.name);

    let interactor = get_server_interactor(&node.name)?;

    // Ensure postgres binaries are installed first
    install_postgres(&*interactor, &pg_version)?;

    // Install & configure etcd (configure only, do NOT start yet)
    install_etcd(&*interactor)?;
    setup_etcd(&*interactor, &node, &pg_nodes)?;

    // Stop & disable standard postgresql systemd service
    println!("\tStopping and disabling standard PostgreSQL service...");
    let _stop_res = interactor.stop_service("postgresql");
    let _disable_pg_res = interactor.disable_service("postgresql");

    let patroni_configured = interactor
        .cmd("test -f /etc/patroni/config.yml")
        .map(|out| out.exit_code == 0)
        .unwrap_or(false);

    if !patroni_configured {
        // Stop and kill Patroni before cleaning up config and bootstrapping
        let _stop_patroni_res = interactor.stop_service("patroni");
        let _ = interactor.cmd("sudo pkill -9 -u postgres postgres");

        // Backup existing postgres main directory
        backup_postgres_dir(&pg_version, &*interactor)?;
    }

    // Configure WAL archive directory
    interactor.cmd("sudo mkdir -p /var/lib/postgresql/wal_archive")?;
    interactor.cmd("sudo chown postgres:postgres /var/lib/postgresql/wal_archive")?;
    interactor.cmd("sudo chmod 700 /var/lib/postgresql/wal_archive")?;

    let patroni_config_changed =
        install_patroni(&pg_version, &replica_pass, &pg_nodes, &node, &*interactor)?;

    // Wait for all nodes to finish configuration/installation
    barrier_installed.wait();

    // --- Phase 2: Start etcd ---
    start_etcd(&node, &*interactor)?;

    // Wait for all nodes to start etcd
    barrier_etcd_started.wait();

    // --- Wait for quorum (only first node runs it) ---
    if is_first_node {
        wait_for_etcd_cluster(&*interactor, &pg_nodes, 60)?;

        // 2. Clear DCS (etcd) keys for the cluster to prevent conflicts
        println!("\tClearing DCS cluster state...");
        pg_clear_dcs_state(&*interactor);
    }

    // Wait for first node to complete quorum check before starting Patroni
    barrier_quorum.wait();

    // --- Phase 3: Start Patroni ---
    // Skip restart if config is unchanged and Patroni REST API is already healthy.
    let patroni_already_healthy = !patroni_config_changed
        && interactor
            .cmd("curl -s -o /dev/null -w \"%{http_code}\" http://127.0.0.1:8008/health")
            .map(|o| o.stdout.trim() == "200")
            .unwrap_or(false);

    if patroni_already_healthy {
        println!(
            "\tPatroni already healthy on node {}, skipping restart",
            node.name
        );
    } else {
        println!("\tStarting Patroni on node {}...", node.name);
        interactor.restart_service("patroni --no-block")?;

        // Check if Patroni is running (with a 30s timeout since it can be "activating" initially)
        let is_active = interactor.wait_for_service_start("patroni", 30)?;

        if !is_active {
            println!(
                "Error: patroni is not running on node {}. Fetching logs...",
                node.name
            );
            let log_out = interactor.cmd("sudo journalctl -xeu patroni.service -n 100 -o cat")?;

            println!("\n===BEGIN PATRONI LOGS ON {}===\n", node.name);
            println!("{}", log_out.stdout);
            println!("\n===END PATRONI LOGS ON {}===\n", node.name);

            anyhow::bail!("Patroni failed to start on node {}", node.name);
        } else {
            println!("\tPatroni started successfully on node {}", node.name);
        }
    }

    println!(
        "\tSetting up automated cron backups on node {}...",
        node.name
    );
    configure_postgres_cron_backup(
        &*interactor,
        &pg_version,
        &replica_pass,
        &schedule,
        &s3_config,
    )?;

    Ok((node.name, interactor))
}
