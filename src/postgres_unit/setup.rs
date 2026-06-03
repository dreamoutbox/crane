use crate::{
    config::{self, PostgresDbConfig, PostgresUserConfig},
    etcd_unit::{install_etcd, setup_etcd, start_etcd, wait_for_etcd_quorum},
    haproxy_unit::haproxy::setup_haproxy_on_each_nodes_wrapper,
    helper::config::config_get_nodes,
    postgres_unit::{
        helper::{
            configure_postgres_cron_backup, get_pg_version, get_postgres_backup_schedule, get_postgres_configs, get_replica_pass, pg_clear_dcs_state, pg_wait_all_replicas, postgres_get_primary
        },
        install::install_postgres,
        patroni::install_patroni,
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

        let handle = tokio::task::spawn_blocking(
            move || -> anyhow::Result<(String, std::sync::Arc<dyn ServerInteractor + Send + Sync>)> {
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
                    wait_for_etcd_quorum(&*interactor, &pg_nodes, 40)?;
            
                    // 2. Clear DCS (etcd) keys for the cluster to prevent conflicts
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
                        let log_out =
                            interactor.cmd("sudo journalctl -xeu patroni.service -n 100 -o cat")?;

                        println!("\n===BEGIN PATRONI LOGS ON {}===\n", node.name);
                        println!("{}", log_out.stdout);
                        println!("\n===END PATRONI LOGS ON {}===\n", node.name);

                        anyhow::bail!("Patroni failed to start on node {}", node.name);
                    } else {
                        println!("\tPatroni started successfully on node {}", node.name);
                    }
                }

                println!("\tSetting up automated cron backups on node {}...", node.name);
                configure_postgres_cron_backup(&*interactor, &pg_version, &replica_pass, &schedule, &s3_config)?;

                Ok((node.name, interactor))
            },
        );

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

    // println!("\tWaiting for replica nodes to join the cluster...");
    pg_wait_all_replicas(&*leader_interactor, &pg_nodes);

    let elapsed = start.elapsed();
    println!(
        "\n\tPostgreSQL cluster health check completed (took {}s)",
        elapsed.as_secs()
    );

    // 5. Setup HAProxy on all app nodes
    setup_haproxy_on_each_nodes_wrapper(config, app_nodes).await?;

    Ok(())
}

 

pub async fn setup_postgres_primary(
    interactor: std::sync::Arc<dyn ServerInteractor + Send + Sync>,
    _version: &str,
    _replica_pass: &str,
    db_configs: &[PostgresDbConfig],
    user_configs: &[PostgresUserConfig],
    _config: &crate::config::Config,
) -> anyhow::Result<()> {
    println!("\n\tProvisioning PostgreSQL databases and users on Patroni leader...");

    let interactor_clone = interactor.clone();
    let db_configs = db_configs.to_vec();
    let user_configs = user_configs.to_vec();

    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let run_psql = |cmd: &str| -> anyhow::Result<crate::ssh::CmdOutput> {
            let out = interactor_clone.cmd(cmd)?;
            if out.exit_code != 0 {
                anyhow::bail!("psql command failed: {}\nStderr: {}", cmd, out.stderr);
            }
            Ok(out)
        };

        // Idempotently create databases sequentially
        for db in &db_configs {
            println!("\n\tSetting up database '{}'...", db.name);

            let check_db_sql = format!("SELECT 1 FROM pg_database WHERE datname = '{}'", db.name);
            let db_exists = run_psql(&format!(
                "sudo -u postgres psql -t -A -c \"{}\"",
                check_db_sql
            ))?;

            if db_exists.stdout.trim() != "1" {
                run_psql(&format!(
                    "sudo -u postgres psql -c \"CREATE DATABASE {};\"",
                    db.name
                ))?;
            }
        }

        // Idempotently create/remove users and grant/revoke privileges sequentially
        for user in &user_configs {
            let user_state = user.state.as_deref().unwrap_or("present");

            println!("\tuser {} state is {}", user.user, user_state);

            if user_state == "absent" {
                println!("\tRemoving user '{}'...", user.user);

                for db_ref in &user.databases {
                    let db_name = db_configs
                        .iter()
                        .find(|d| &d.name == db_ref)
                        .map(|d| d.name.as_str())
                        .unwrap_or(db_ref);

                    println!(
                        "\tRevoking privileges for user '{}' on database '{}'...",
                        user.user, db_name
                    );

                    let _ = interactor_clone.cmd(&format!(
                        "sudo -u postgres psql -d {} -c \"REVOKE ALL ON SCHEMA public FROM {};\"",
                        db_name, user.user
                    ));

                    let _ = interactor_clone.cmd(&format!(
                        "sudo -u postgres psql -c \"REVOKE ALL PRIVILEGES ON DATABASE {} FROM {};\"",
                        db_name, user.user
                    ));
                }

                run_psql(&format!(
                    "sudo -u postgres psql -c \"DROP ROLE IF EXISTS {};\"",
                    user.user
                ))?;
            } else if user_state == "present" {
                println!("\tSetting up user '{}'...", user.user);

                // Write SQL to temp file to avoid shell quoting issues with $$ and newlines
                let password = user.password.as_deref().unwrap_or("").replace('\'', "''");
                let user_sql = format!(
                    "DO $crane$\n\
                     BEGIN\n\
                         IF NOT EXISTS (SELECT FROM pg_catalog.pg_roles WHERE rolname = '{}') THEN\n\
                             CREATE ROLE {} WITH PASSWORD '{}' LOGIN;\n\
                         ELSE\n\
                             ALTER ROLE {} WITH PASSWORD '{}';\n\
                         END IF;\n\
                     END $crane$;",
                    user.user, user.user, password, user.user, password
                );
                let tmp_sql = format!("/tmp/crane_user_{}.sql", user.user);
                interactor_clone.create_file(&tmp_sql, &user_sql)?;
                let psql_res = run_psql(&format!("sudo -u postgres psql -f '{}'", tmp_sql));
                let _ = interactor_clone.cmd(&format!("sudo rm -f '{}'", tmp_sql));
                psql_res?;

                for db_ref in &user.databases {
                    let db_name = db_configs
                        .iter()
                        .find(|d| &d.name == db_ref)
                        .map(|d| d.name.as_str())
                        .unwrap_or(db_ref);

                    println!(
                        "\tGranting access for user '{}' to database '{}'...",
                        user.user, db_name
                    );

                    run_psql(&format!(
                        "sudo -u postgres psql -c \"GRANT ALL PRIVILEGES ON DATABASE {} TO {};\"",
                        db_name, user.user
                    ))?;

                    run_psql(&format!(
                        "sudo -u postgres psql -d {} -c \"GRANT ALL ON SCHEMA public TO {};\"",
                        db_name, user.user
                    ))?;
                }
            } else {
                anyhow::bail!("unknown user state: {}", user_state);
            }
        }


        Ok(())
    })
    .await??;

    Ok(())
}

pub fn backup_postgres_dir(
    pg_version: &String,
    interactor: &dyn ServerInteractor,
) -> Result<(), anyhow::Error> {
    let timestamp_out = interactor.cmd("date +%s")?;
    let unix_timestamp = timestamp_out.stdout.trim().to_string();
    if unix_timestamp.is_empty() {
        anyhow::bail!("Failed to generate UNIX timestamp for backup path");
    }

    let backup_parent = format!(
        "/backup/{}/var/lib/postgresql/{}",
        unix_timestamp, *pg_version
    );

    let old_main_dir = format!("/var/lib/postgresql/{}/main", *pg_version);
    let backup_main_dir = format!("{}/main", backup_parent);
    let dir_exists = interactor
        .cmd(&format!("test -d {}", old_main_dir))
        .map(|out| out.exit_code == 0)
        .unwrap_or(false);
    if dir_exists {
        println!(
            "\tBacking up old postgres data directory {} to {}",
            old_main_dir, backup_main_dir
        );
        interactor.cmd(&format!("sudo mkdir -p {}", backup_parent))?;
        interactor.cmd(&format!("sudo mv {} {}", old_main_dir, backup_main_dir))?;
    }

    let failed_main_dir = format!("/var/lib/postgresql/{}/main.failed", *pg_version);
    let backup_failed_dir = format!("{}/main.failed", backup_parent);
    let failed_exists = interactor
        .cmd(&format!("test -d {}", failed_main_dir))
        .map(|out| out.exit_code == 0)
        .unwrap_or(false);
    if failed_exists {
        println!(
            "\tBacking up failed data directory {} to {}...",
            failed_main_dir, backup_failed_dir
        );
        interactor.cmd(&format!("sudo mkdir -p {}", backup_parent))?;
        interactor.cmd(&format!(
            "sudo mv {} {}",
            failed_main_dir, backup_failed_dir
        ))?;
    }

    Ok(())
}
