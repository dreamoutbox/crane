use crate::{
    config::{self, PostgresDbConfig, PostgresUserConfig},
    helper::keys::find_private_key_for_user,
    postgres_unit::{
        helper::{
            configure_postgres_backup, configure_postgres_primary_rules, get_postgres_configs,
        },
        install::install_postgres,
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
        .and_then(|pg| pg.get("version"))
        .and_then(|val| val.as_str())
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

    if !pg_nodes.is_empty() {
        let primary_node = &pg_nodes[0];
        let follower_ips: Vec<String> = pg_nodes[1..]
            .iter()
            .map(|n| n.internal_ip.clone())
            .collect();
        let app_node_ips: Vec<String> = config
            .nodes
            .iter()
            .filter(|n| n.roles.contains(&"app".to_string()))
            .map(|n| n.internal_ip.clone())
            .collect();
        let (db_configs, user_configs) = get_postgres_configs(config);

        // 1. Setup primary node
        println!(
            "Setting up primary PostgreSQL node at {}...",
            primary_node.name
        );

        let private_key = find_private_key_for_user(&primary_node.user, config)?;
        let ssh_primary = SSHSession::new(
            primary_node.host.clone(),
            primary_node.user.clone(),
            private_key,
            Some(primary_node.port),
        );
        let interactor = get_server_interactor(ssh_primary)?;
        setup_postgres_primary(
            &*interactor,
            &pg_version,
            "replicator",
            &replica_pass,
            &follower_ips,
            &app_node_ips,
            &db_configs,
            &user_configs,
            config,
            dot_env,
        )?;

        // 2. Setup follower nodes
        for follower_node in &pg_nodes[1..] {
            println!(
                "Setting up follower PostgreSQL node at {}...",
                follower_node.name
            );
            let private_key = find_private_key_for_user(&follower_node.user, config)?;
            let ssh = SSHSession::new(
                follower_node.host.clone(),
                follower_node.user.clone(),
                private_key,
                Some(follower_node.port),
            );
            let follower_interactor = get_server_interactor(ssh)?;
            setup_postgres_follower(
                &*follower_interactor,
                &pg_version,
                &primary_node.internal_ip,
                "replicator",
                &replica_pass,
                &app_node_ips,
            )?;
        }

        // 3. Setup HAProxy on all vps nodes
        for app_node in app_nodes {
            println!("\n\tSetting up HAProxy on app node {}...", app_node.name);
            let private_key = find_private_key_for_user(&app_node.user, config)?;

            let ssh = SSHSession::new(
                app_node.host.clone(),
                app_node.user.clone(),
                private_key,
                Some(app_node.port),
            );
            let interactor = get_server_interactor(ssh)?;

            crate::postgres_unit::haproxy::setup_haproxy(
                &*interactor,
                &primary_node.internal_ip,
                &follower_ips,
            )?;
        }
    }

    Ok(())
}

pub fn setup_postgres_primary(
    interactor: &dyn ServerInteractor,
    version: &str,
    replica_user: &str,
    replica_pass: &str,
    follower_ips: &[String],
    app_node_ips: &[String],
    db_configs: &[PostgresDbConfig],
    user_configs: &[PostgresUserConfig],
    config: &crate::config::Config,
    dot_env: &std::collections::HashMap<String, String>,
) -> anyhow::Result<()> {
    println!("\tSetting up PostgreSQL primary node...");
    install_postgres(interactor, version)?;

    // Verify postgres is running. if not try start it. if can't start then print error and exit.
    crate::postgres_unit::helper::ensure_postgres_running(interactor, version);

    println!("\tConfiguring primary replication and local trust rules...");
    configure_postgres_primary_rules(
        interactor,
        version,
        replica_user,
        follower_ips,
        app_node_ips,
    )?;

    // 4. Idempotently create replicator user
    println!("\tCreating replication user '{}'...", replica_user);
    let replicator_sql = format!(
        "DO \\$\\$ BEGIN IF NOT EXISTS (SELECT FROM pg_catalog.pg_roles WHERE rolname = '{}') THEN CREATE ROLE {} WITH REPLICATION PASSWORD '{}' LOGIN; END IF; END \\$\\$;",
        replica_user, replica_user, replica_pass
    );
    interactor.cmd(&format!("sudo -u postgres psql -c \"{}\"", replicator_sql))?;

    // 5. Idempotently create databases
    for db in db_configs {
        println!("\n\tSetting up database '{}'...", db.db_name);
        let check_db_sql = format!("SELECT 1 FROM pg_database WHERE datname = '{}'", db.db_name);
        let db_exists = interactor.cmd(&format!(
            "sudo -u postgres psql -t -A -c \"{}\"",
            check_db_sql
        ))?;
        if db_exists.stdout.trim() != "1" {
            interactor.cmd(&format!(
                "sudo -u postgres psql -c \"CREATE DATABASE {};\"",
                db.db_name
            ))?;
        }
    }

    // 6. Idempotently create users and grant permissions
    for user in user_configs {
        println!("\tSetting up user '{}'...", user.user);

        let user_sql = format!(
            "DO \\$\\$ \
             BEGIN \
                 IF NOT EXISTS (SELECT FROM pg_catalog.pg_roles WHERE rolname = '{}') THEN \
                     CREATE ROLE {} WITH PASSWORD '{}' LOGIN; \
                 ELSE \
                     ALTER ROLE {} WITH PASSWORD '{}'; \
                 END IF; \
             END \\$\\$;",
            user.user,
            user.user,
            user.password.as_deref().unwrap_or(""),
            user.user,
            user.password.as_deref().unwrap_or("")
        );
        interactor.cmd(&format!("sudo -u postgres psql -c \"{}\"", user_sql))?;

        for db_ref in &user.databases {
            // Find db_name corresponding to db_ref (which could be key or db_name itself)
            let db_name = db_configs
                .iter()
                .find(|d| &d.name == db_ref || &d.db_name == db_ref)
                .map(|d| d.db_name.as_str())
                .unwrap_or(db_ref);

            println!(
                "\tGranting access for user '{}' to database '{}'...",
                user.user, db_name
            );

            // Grant privileges on the database
            interactor.cmd(&format!(
                "sudo -u postgres psql -c \"GRANT ALL PRIVILEGES ON DATABASE {} TO {};\"",
                db_name, user.user
            ))?;

            // Grant privileges on schema public inside that database
            interactor.cmd(&format!(
                "sudo -u postgres psql -d {} -c \"GRANT ALL ON SCHEMA public TO {};\"",
                db_name, user.user
            ))?;
        }
    }

    configure_postgres_backup(interactor, version, replica_pass, config, dot_env)?;

    Ok(())
}

pub fn setup_postgres_follower(
    interactor: &dyn ServerInteractor,
    version: &str,
    primary_ip: &str,
    replica_user: &str,
    replica_pass: &str,
    app_node_ips: &[String],
) -> anyhow::Result<()> {
    println!(
        "Setting up PostgreSQL follower node replicating from {}...",
        primary_ip
    );
    install_postgres(interactor, version)?;

    // Verify postgres is running. if not try start it. if can't start then print error and exit.
    crate::postgres_unit::helper::ensure_postgres_running(interactor, version);

    println!("\tStopping PostgreSQL cluster on follower...");
    let pg_ctl = format!("/usr/lib/postgresql/{}/bin/pg_ctl", version);
    let stop_cmd = format!(
        "sudo -u postgres {} -D /var/lib/postgresql/{}/main stop > /dev/null 2>&1 < /dev/null",
        pg_ctl, version
    );
    let _ = interactor.cmd(&stop_cmd);

    println!("\tClearing follower PostgreSQL data directory...");
    interactor.cmd(&format!("sudo rm -rf /var/lib/postgresql/{}/main", version))?;

    println!("\tRunning pg_basebackup from primary...");
    let backup_cmd = format!(
        "sudo -u postgres PGPASSWORD='{}' pg_basebackup -h {} -D /var/lib/postgresql/{}/main/ -U {} -c fast -P -R",
        replica_pass, primary_ip, version, replica_user
    );
    interactor.cmd(&backup_cmd)?;

    // Configure local trust on the follower node pg_hba.conf
    let pg_hba_path = format!("/etc/postgresql/{}/main/pg_hba.conf", version);
    println!(
        "\tConfiguring local trust on follower pg_hba.conf... (file: {})",
        pg_hba_path
    );
    if let Ok(existing_hba) = interactor.cmd(&format!("sudo cat '{}'", pg_hba_path)) {
        let mut updated_hba = existing_hba.stdout.clone();
        updated_hba = updated_hba.replace(
            "local   all             postgres                                peer",
            "local   all             postgres                                trust",
        );
        updated_hba = updated_hba.replace(
            "local   all             all                                     peer",
            "local   all             all                                     trust",
        );
        updated_hba = updated_hba.replace(
            "host    all             all             127.0.0.1/32            scram-sha-256",
            "host    all             all             127.0.0.1/32            trust",
        );
        updated_hba = updated_hba.replace(
            "host    all             all             ::1/128                 scram-sha-256",
            "host    all             all             ::1/128                 trust",
        );

        let mut new_rules = String::new();
        for app_ip in app_node_ips {
            let rule = format!("host all all {}/32 scram-sha-256", app_ip);
            if !updated_hba.contains(&rule) {
                new_rules.push_str(&format!("{}\n", rule));
            }
        }

        if !new_rules.is_empty() {
            if !updated_hba.ends_with('\n') {
                updated_hba.push('\n');
            }
            updated_hba.push_str("\n# Crane replication & client connections\n");
            updated_hba.push_str(&new_rules);
        }

        if updated_hba != existing_hba.stdout {
            interactor.create_file(&pg_hba_path, &updated_hba)?;
            interactor.cmd(&format!("sudo chown postgres:postgres '{}'", pg_hba_path))?;
            interactor.cmd(&format!("sudo chmod 640 '{}'", pg_hba_path))?;
        }
    }

    println!("\tConfiguring postgresql.conf on follower...");
    crate::postgres_unit::helper::configure_postgresql_conf(interactor, version)?;

    println!("\tStarting PostgreSQL cluster on follower...");
    let pg_ctl = format!("/usr/lib/postgresql/{}/bin/pg_ctl", version);
    let start_cmd = format!(
        "sudo -u postgres {} -D /var/lib/postgresql/{}/main -o \"-c config_file=/etc/postgresql/{}/main/postgresql.conf -c restore_command=false\" start > /dev/null 2>&1 < /dev/null",
        pg_ctl, version, version
    );
    interactor.cmd(&start_cmd)?;

    // Verify postgres is running. if not try start it. if can't start then print error and exit.
    crate::postgres_unit::helper::ensure_postgres_running(interactor, version);

    Ok(())
}
