use crate::{
    config::{self, PostgresDbConfig, PostgresUserConfig},
    haproxy_unit::haproxy::setup_haproxy_each_nodes_wrapper,
    helper::keys::find_private_key_for_user,
    postgres_unit::{
        etcd,
        helper::{configure_postgres_backup, connect_to_node, get_postgres_configs},
        install::install_postgres,
        patroni::install_patroni,
    },
    server_interactor::{get_server_interactor, server_interactor_trait::ServerInteractor},
    ssh::SSHSession,
};

pub fn postgres_setup_wrapper(
    config: &config::Config,
    dot_env: &std::collections::HashMap<String, String>,
    app_nodes: &Vec<config::NodeConfig>,
) -> Result<(), anyhow::Error> {
    let pg_version = config
        .db
        .as_ref()
        .and_then(|db| db.postgres.as_ref())
        .map(|pg| pg.version.as_str())
        .unwrap_or("16")
        .to_string();
    let replica_pass = dot_env
        .get("POSTGRES_PASSWORD")
        .cloned()
        .unwrap_or_else(|| "repl_password".to_string());
    let pg_nodes: Vec<_> = config
        .nodes
        .iter()
        .filter(|n| n.roles.contains(&"postgres".to_string()))
        .cloned()
        .collect();

    if pg_nodes.is_empty() {
        return Ok(());
    }

    // Phase 1: Install & configure etcd and Patroni on all postgres nodes (no starts yet)
    for node in &pg_nodes {
        println!("Configuring node {}...", node.name);

        let private_key = find_private_key_for_user(&node.user, config)?;
        let ssh = SSHSession::new(
            node.host.clone(),
            node.user.clone(),
            private_key,
            Some(node.port),
        );
        let interactor = get_server_interactor(ssh)?;

        // Ensure postgres binaries are installed first
        install_postgres(&*interactor, &pg_version)?;

        // Install & configure etcd (configure only, do NOT start yet)
        etcd::install_etcd(&*interactor)?;
        etcd::setup_etcd(&*interactor, node, &pg_nodes)?;

        // Stop & disable standard postgresql systemd service
        println!("\tStopping and disabling standard PostgreSQL service...");
        let _stop_res = interactor.cmd("sudo systemctl stop postgresql");
        // dbg!(&stop_res);

        // let stop2_res = interactor.cmd(&format!(
        //     "sudo systemctl stop postgresql@{}-main",
        //     pg_version
        // ));
        // dbg!(&stop2_res);

        // let kill_res = interactor.cmd("sudo pkill -u postgres -f postgres");
        // dbg!(&kill_res);

        // std::thread::sleep(std::time::Duration::from_millis(500));

        let _disable_pg_res = interactor.cmd("sudo systemctl disable postgresql");
        // dbg!(&disable_pg_res);

        // let _disable2_pg_res = interactor.cmd(&format!(
        //     "sudo systemctl disable postgresql@{}-main",
        //     pg_version
        // ));
        // dbg!(&disable2_pg_res);

        // Stop and kill Patroni before cleaning up config and bootstrapping
        let _stop_patroni_res = interactor.cmd("sudo systemctl stop patroni");
        // dbg!(&stop_patroni_res);

        // let kill_patroni_res = interactor.cmd("sudo pkill -f patroni");
        // dbg!(&kill_patroni_res);

        // Backup existing postgres main directory
        //or failed boot directory if present
        backup_postgres_dir(&pg_version, &interactor)?;

        // Configure WAL archive directory
        interactor.cmd("sudo mkdir -p /var/lib/postgresql/wal_archive")?;
        interactor.cmd("sudo chown postgres:postgres /var/lib/postgresql/wal_archive")?;
        interactor.cmd("sudo chmod 700 /var/lib/postgresql/wal_archive")?;

        install_patroni(&pg_version, &replica_pass, &pg_nodes, node, interactor)?;
    }

    // Phase 2: Start etcd on all nodes simultaneously so they form quorum together
    println!("\tStarting etcd on all nodes...");
    for node in &pg_nodes {
        let private_key = find_private_key_for_user(&node.user, config)?;
        let ssh = SSHSession::new(
            node.host.clone(),
            node.user.clone(),
            private_key,
            Some(node.port),
        );
        let interactor = get_server_interactor(ssh)?;
        etcd::start_etcd(&*interactor)?;
    }

    // Give etcd time to elect a leader and form quorum
    if !pg_nodes.is_empty() {
        let first_node = &pg_nodes[0];
        let private_key = find_private_key_for_user(&first_node.user, config)?;
        let ssh = SSHSession::new(
            first_node.host.clone(),
            first_node.user.clone(),
            private_key,
            Some(first_node.port),
        );
        let interactor = get_server_interactor(ssh)?;

        etcd::wait_for_etcd_quorum(&*interactor, 10)?;
    }

    // Phase 3: Start Patroni on all nodes simultaneously
    println!("\tStarting Patroni on all nodes...");
    // dbg!(&pg_nodes);

    for node in &pg_nodes {
        let private_key = find_private_key_for_user(&node.user, config)?;
        let ssh = SSHSession::new(
            node.host.clone(),
            node.user.clone(),
            private_key,
            Some(node.port),
        );
        let interactor = get_server_interactor(ssh)?;

        println!("\tStarting Patroni on node {}...", node.name);
        interactor.cmd("sudo systemctl restart patroni --no-block")?;

        // Check if Patroni is running (with a 10s timeout since it can be "activating" initially)
        let start_time = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(10);
        let mut is_active = false;

        while start_time.elapsed() < timeout {
            let active_out = interactor.cmd("sudo systemctl is-active patroni")?;
            // dbg!(&active_out);

            if active_out.stdout.trim() == "active" {
                is_active = true;
                break;
            }

            std::thread::sleep(std::time::Duration::from_millis(500));
        }

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

    // 2. Wait for Patroni leader election
    println!("\tWaiting for Patroni leader election...");
    let mut leader_node = None;
    for _ in 0..100 {
        if let Ok(Some(leader)) = crate::postgres_unit::helper::postgres_get_leader(config) {
            leader_node = Some(leader);
            break;
        }

        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    let leader = leader_node
        .ok_or_else(|| anyhow::anyhow!("Timeout waiting for PostgreSQL Patroni leader election"))?;
    println!("\tDiscovered Patroni leader at node: {}", leader.name);

    let follower_ips: Vec<String> = pg_nodes
        .iter()
        .filter(|n| n.internal_ip != leader.internal_ip)
        .map(|n| n.internal_ip.clone())
        .collect();
    println!("\tFollower: {:?}", follower_ips);

    // 3. Provision database schema and users on the dynamic leader
    let leader_interactor = connect_to_node(&leader, config)?;
    let (db_configs, user_configs) = get_postgres_configs(config);

    setup_postgres_primary(
        &*leader_interactor,
        &pg_version,
        &replica_pass,
        &db_configs,
        &user_configs,
        config,
    )?;

    // 4. Setup HAProxy on all app nodes
    setup_haproxy_each_nodes_wrapper(config, app_nodes)?;

    Ok(())
}

pub fn setup_postgres_primary(
    interactor: &dyn ServerInteractor,
    version: &str,
    replica_pass: &str,
    db_configs: &[PostgresDbConfig],
    user_configs: &[PostgresUserConfig],
    config: &crate::config::Config,
) -> anyhow::Result<()> {
    println!("\tProvisioning PostgreSQL databases and users on Patroni leader...");

    // Idempotently create databases
    for db in db_configs {
        println!("\n\tSetting up database '{}'...", db.name);

        let check_db_sql = format!("SELECT 1 FROM pg_database WHERE datname = '{}'", db.name);
        let db_exists = interactor.cmd(&format!(
            "sudo -u postgres psql -t -A -c \"{}\"",
            check_db_sql
        ))?;

        if db_exists.stdout.trim() != "1" {
            interactor.cmd(&format!(
                "sudo -u postgres psql -c \"CREATE DATABASE {};\"",
                db.name
            ))?;
        }
    }

    // Idempotently create/remove users and grant/revoke privileges
    for user in user_configs {
        let user_state = user.state.as_deref().unwrap_or("present");

        println!("\tuser {} state is {}", user.user, user_state);

        if user_state == "absent" {
            println!("\tRemoving user '{}'...", user.user);

            for db_ref in &user.databases {
                let db_name = db_configs
                    .iter()
                    .find(|d| &d.name == db_ref || &d.name == db_ref)
                    .map(|d| d.name.as_str())
                    .unwrap_or(db_ref);

                println!(
                    "\tRevoking privileges for user '{}' on database '{}'...",
                    user.user, db_name
                );

                let _ = interactor.cmd(&format!(
                    "sudo -u postgres psql -d {} -c \"REVOKE ALL ON SCHEMA public FROM {};\"",
                    db_name, user.user
                ));

                let _ = interactor.cmd(&format!(
                    "sudo -u postgres psql -c \"REVOKE ALL PRIVILEGES ON DATABASE {} FROM {};\"",
                    db_name, user.user
                ));
            }

            interactor.cmd(&format!(
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
            interactor.create_file(&tmp_sql, &user_sql)?;
            interactor.cmd(&format!("sudo -u postgres psql -f '{}'", tmp_sql))?;
            interactor.cmd(&format!("sudo rm -f '{}'", tmp_sql))?;

            for db_ref in &user.databases {
                let db_name = db_configs
                    .iter()
                    .find(|d| &d.name == db_ref || &d.name == db_ref)
                    .map(|d| d.name.as_str())
                    .unwrap_or(db_ref);

                println!(
                    "\tGranting access for user '{}' to database '{}'...",
                    user.user, db_name
                );

                interactor.cmd(&format!(
                    "sudo -u postgres psql -c \"GRANT ALL PRIVILEGES ON DATABASE {} TO {};\"",
                    db_name, user.user
                ))?;

                interactor.cmd(&format!(
                    "sudo -u postgres psql -d {} -c \"GRANT ALL ON SCHEMA public TO {};\"",
                    db_name, user.user
                ))?;
            }
        } else {
            anyhow::bail!("unknown user state: {}", user_state);
        }
    }

    configure_postgres_backup(interactor, version, replica_pass, config)?;

    Ok(())
}

pub fn backup_postgres_dir(
    pg_version: &String,
    interactor: &Box<dyn ServerInteractor>,
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
            "\tBacking up existing data directory {} to {}",
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
