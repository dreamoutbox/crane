use crate::{
    config::{
        self, get_pg_replica_pass, get_postgres_backup_schedule_config,
        get_postgres_dbs_and_users_config,
    },
    etcd_unit::etcd_wait_for_cluster,
    helper::config::config_get_nodes,
    postgres_unit::{
        helper::{backup_pg_dir, get_pg_version, pg_cluster_wait_all_nodes_ready, pg_get_primary},
        setup_postgres_primary::setup_postgres_primary,
    },
    server_interactor::{get_server_interactor, server_interactor_trait::ServerInteractor},
};

pub async fn postgres_setup_wrapper(config: &config::Config) -> Result<(), anyhow::Error> {
    let pg_version = get_pg_version(&config);
    let replica_pass = get_pg_replica_pass(&config);

    let schedule = get_postgres_backup_schedule_config(config);
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
        if let Ok(Some(leader)) = pg_get_primary(config) {
            primary_node = Some(leader);
            break;
        }

        std::thread::sleep(std::time::Duration::from_millis(1000));
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
    let (db_configs, user_configs) = get_postgres_dbs_and_users_config(config);

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

    // Ensure postgres installed first
    interactor.install_postgres(&pg_version)?;

    // Install & configure etcd (configure only, do NOT start yet)
    interactor.setup_etcd(&node, &pg_nodes)?;

    // Stop & disable standard postgresql systemd service
    println!(
        "\tStopping and disabling standard PostgreSQL service on node {}...",
        node.name
    );
    let _stop_res = interactor.stop_service("postgresql");
    let _disable_pg_res = interactor.disable_service("postgresql");

    let patroni_configured = interactor
        .exists(&interactor.server_paths().patroni_config_path)
        .unwrap_or(false);

    if !patroni_configured {
        let _stop_patroni_res = interactor.stop_service("patroni");
        let _ = interactor.kill_postgres_processes();

        // Backup existing postgres main directory
        backup_pg_dir(&pg_version, &*interactor)?;
    }

    // Configure WAL archive directory
    interactor.mkdir("/var/lib/postgresql/wal_archive")?;
    interactor.chown("/var/lib/postgresql/wal_archive", "postgres", "postgres")?;
    interactor.chmod("/var/lib/postgresql/wal_archive", "700")?;

    let patroni_config_changed =
        interactor.setup_patroni(&node, &pg_version, &replica_pass, &pg_nodes)?;

    // Wait for all nodes to finish configuration/installation
    barrier_installed.wait();

    // --- Phase 2: Start etcd ---
    interactor.start_etcd(&node)?;

    // Wait for all nodes to start etcd
    barrier_etcd_started.wait();

    // --- Wait for quorum (only first node runs it) ---
    if is_first_node {
        println!("\tWaiting for etcd ready on all nodes...");
        etcd_wait_for_cluster(&*interactor, &pg_nodes, 60)?;
        println!("\tEtcd cluster ready.");
    }

    // Wait for first node to complete quorum check before starting Patroni
    barrier_quorum.wait();

    // --- Phase 3: Start Patroni ---
    // Skip restart if config is unchanged and Patroni REST API is already healthy.
    let patroni_already_healthy = !patroni_config_changed
        && interactor
            .check_http_status("http://127.0.0.1:8008/health")
            .map(|code| code == 200)
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

    crate::postgres_unit::cron::configure_postgres_cron_backup(
        &*interactor,
        &pg_version,
        &replica_pass,
        &schedule,
        &s3_config,
    )?;

    Ok((node.name, interactor))
}
