use crate::{
    config::{self, PostgresDbConfig, PostgresUserConfig},
    helper::keys::{find_private_key_for_user, get_any_private_key},
    postgres_unit::{
        helper::{get_postgres_backup_schedule, interval_to_cron},
        install::install_postgres,
        python_backup_script::PYTHON_BACKUP_SCRIPT,
    },
    server_interactor::server_interactor_trait::ServerInteractor,
    ssh::SSHSession,
};

pub fn postgres_setup_wrapper(
    get_interactor: fn(SSHSession) -> anyhow::Result<Box<dyn ServerInteractor>>,
    config: &config::Config,
    dot_env: &std::collections::HashMap<String, String>,
    app_nodes: Vec<config::NodeConfig>,
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

        let private_key = find_private_key_for_user(&primary_node.user, config);
        let private_key = if private_key.is_empty() {
            get_any_private_key(config)
        } else {
            private_key
        };
        let ssh = SSHSession::new(
            primary_node.host.clone(),
            primary_node.user.clone(),
            private_key,
            Some(primary_node.port),
        );
        let interactor = get_interactor(ssh)?;
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
            let private_key = find_private_key_for_user(&follower_node.user, config);
            let private_key = if private_key.is_empty() {
                get_any_private_key(config)
            } else {
                private_key
            };
            let ssh = SSHSession::new(
                follower_node.host.clone(),
                follower_node.user.clone(),
                private_key,
                Some(follower_node.port),
            );
            let interactor = get_interactor(ssh)?;
            setup_postgres_follower(
                &*interactor,
                &pg_version,
                &primary_node.internal_ip,
                "replicator",
                &replica_pass,
            )?;
        }

        // 3. Setup HAProxy on all vps nodes
        for app_node in &app_nodes {
            println!("\tSetting up HAProxy on app node {}...", app_node.name);
            let private_key = find_private_key_for_user(&app_node.user, config);
            let private_key = if private_key.is_empty() {
                get_any_private_key(config)
            } else {
                private_key
            };
            let ssh = SSHSession::new(
                app_node.host.clone(),
                app_node.user.clone(),
                private_key,
                Some(app_node.port),
            );
            let interactor = get_interactor(ssh)?;

            crate::postgres_unit::haproxy::setup_haproxy(
                &*interactor,
                &primary_node.internal_ip,
                &follower_ips,
            )?;
        }
    }

    Ok(())
}

// Parses database and user configs from TOML structure
pub fn get_postgres_configs(
    config: &crate::config::Config,
) -> (Vec<PostgresDbConfig>, Vec<PostgresUserConfig>) {
    let mut db_configs = Vec::new();
    let mut user_configs = std::collections::HashMap::new();

    if let Some(ref db) = config.db {
        if let Some(ref pg_map) = db.postgres {
            // 1. Parse databases
            for (key, val) in pg_map {
                if key == "enabled" || key == "version" || key == "users" || key == "backup" {
                    continue;
                }

                if let Some(table) = val.as_table() {
                    let db_name = table
                        .get("db_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    if !db_name.is_empty() {
                        db_configs.push(PostgresDbConfig {
                            name: key.clone(),
                            db_name,
                        });
                    }
                }
            }

            // 2. config postgres users
            // println!("\tConfigure postgres users");
            configure_postgres_users(&mut user_configs, pg_map);
        }
    }

    let users = user_configs.into_values().collect();

    (db_configs, users)
}

pub fn configure_postgres_users(
    user_configs: &mut std::collections::HashMap<String, PostgresUserConfig>,
    pg_map: &std::collections::HashMap<String, toml::Value>,
) {
    if let Some(users_val) = pg_map.get("users") {
        // Parse users array
        if let Some(users_arr) = users_val.as_array() {
            for u_val in users_arr {
                if let Some(u_table) = u_val.as_table() {
                    let user = u_table
                        .get("user")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    if !user.is_empty() {
                        let password = u_table
                            .get("password")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());

                        let mut databases = Vec::new();

                        if let Some(db_list_val) = u_table.get("databases") {
                            if let Some(db_arr) = db_list_val.as_array() {
                                for db_item in db_arr {
                                    if let Some(db_name_str) = db_item.as_str() {
                                        databases.push(db_name_str.to_string());
                                    }
                                }
                            }
                        }

                        let user_entry = user_configs.entry(user.clone()).or_insert_with(|| {
                            PostgresUserConfig {
                                user: user.clone(),
                                password,
                                databases: Vec::new(),
                            }
                        });

                        for db_name in databases {
                            if !user_entry.databases.contains(&db_name) {
                                user_entry.databases.push(db_name);
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn configure_postgres_primary_rules(
    interactor: &dyn ServerInteractor,
    version: &str,
    replica_user: &str,
    follower_ips: &[String],
    app_node_ips: &[String],
) -> anyhow::Result<()> {
    // 1. Configure postgresql.conf parameters using ALTER SYSTEM
    interactor.cmd("sudo -u postgres psql -c \"ALTER SYSTEM SET listen_addresses = '*';\"")?;
    interactor.cmd("sudo -u postgres psql -c \"ALTER SYSTEM SET wal_level = 'replica';\"")?;
    interactor.cmd("sudo -u postgres psql -c \"ALTER SYSTEM SET max_wal_senders = 10;\"")?;
    interactor.cmd("sudo -u postgres psql -c \"ALTER SYSTEM SET hot_standby = 'on';\"")?;
    interactor.cmd("sudo -u postgres psql -c \"ALTER SYSTEM SET log_statement = 'mod';\"")?;
    interactor
        .cmd("sudo -u postgres psql -c \"ALTER SYSTEM SET log_min_duration_statement = 0;\"")?;
    interactor.cmd("sudo -u postgres psql -c \"ALTER SYSTEM SET log_line_prefix = '%t [%p]: user=%u db=%d app=%a client=%h ';\"")?;
    interactor.cmd("sudo -u postgres psql -c \"ALTER SYSTEM SET log_destination = 'csvlog';\"")?;
    interactor.cmd("sudo -u postgres psql -c \"ALTER SYSTEM SET logging_collector = 'on';\"")?;
    // WAL archiving for PITR support
    interactor.cmd("sudo mkdir -p /var/lib/postgresql/wal_archive")?;
    interactor.cmd("sudo chown postgres:postgres /var/lib/postgresql/wal_archive")?;
    interactor.cmd("sudo chmod 700 /var/lib/postgresql/wal_archive")?;
    interactor.cmd("sudo -u postgres psql -c \"ALTER SYSTEM SET archive_mode = 'on';\"")?;
    interactor.cmd("sudo -u postgres psql -c \"ALTER SYSTEM SET archive_command = 'cp %p /var/lib/postgresql/wal_archive/%f';\"")?;

    if version.parse::<i32>().unwrap_or(0) >= 17 {
        interactor.cmd("sudo -u postgres psql -c \"ALTER SYSTEM SET summarize_wal = 'on';\"")?;
    }

    // 2. Configure pg_hba.conf
    let pg_hba_path = format!("/etc/postgresql/{}/main/pg_hba.conf", version);
    let existing_hba = interactor.cmd(&format!("sudo cat '{}'", pg_hba_path))?;
    let mut updated_hba = existing_hba.stdout.clone();

    // Allow local connections without password (trust)
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
    for follower in follower_ips {
        let rule = format!(
            "host replication {} {}/32 scram-sha-256",
            replica_user, follower
        );
        if !updated_hba.contains(&rule) {
            new_rules.push_str(&format!("{}\n", rule));
        }
    }
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
        interactor.create_file("/tmp/pg_hba.conf.tmp", &updated_hba)?;
        interactor.cmd(&format!("sudo mv /tmp/pg_hba.conf.tmp '{}'", pg_hba_path))?;
        interactor.cmd(&format!("sudo chown postgres:postgres '{}'", pg_hba_path))?;
        interactor.cmd(&format!("sudo chmod 640 '{}'", pg_hba_path))?;
    }

    // Restart PostgreSQL cluster to apply replication config
    println!("\tRestarting PostgreSQL cluster to apply replication config...");
    let pg_ctl = format!("/usr/lib/postgresql/{}/bin/pg_ctl", version);
    let restart_cmd = format!(
        "sudo -u postgres {} -D /var/lib/postgresql/{}/main -o \"-c config_file=/etc/postgresql/{}/main/postgresql.conf -c restore_command=false\" restart > /dev/null 2>&1 < /dev/null",
        pg_ctl, version, version
    );
    interactor.cmd(&restart_cmd)?;

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
            "DO \\$\\$ BEGIN IF NOT EXISTS (SELECT FROM pg_catalog.pg_roles WHERE rolname = '{}') THEN CREATE ROLE {} WITH PASSWORD '{}' LOGIN; END IF; END \\$\\$;",
            user.user,
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

fn configure_postgres_backup(
    interactor: &dyn ServerInteractor,
    version: &str,
    replica_pass: &str,
    config: &config::Config,
    dot_env: &std::collections::HashMap<String, String>,
) -> anyhow::Result<()> {
    if let Some(schedule) = get_postgres_backup_schedule(config) {
        println!("\tSetting up automated cron backups...");

        // Resolve S3Config
        let s3_config = crate::s3::get_s3_config(config, dot_env)?;

        // Ensure directories exist
        interactor.cmd("sudo mkdir -p /etc/crane /opt/crane /var/lib/postgresql/backups")?;
        interactor.cmd("sudo chown postgres:postgres /var/lib/postgresql/backups")?;
        interactor.cmd("sudo chmod 755 /var/lib/postgresql/backups")?;

        // Write postgres-backup-config.json
        let s3_json_str = format!(
            r#"
{{
  "bucket": "{}",
  "region": "{}",
  "endpoint": {},
  "access_key": "{}",
  "secret_key": "{}",
  "pg_version": "{}",
  "replica_pass": "{}"
}}"#,
            s3_config.bucket,
            s3_config.region,
            s3_config
                .endpoint
                .as_ref()
                .map(|e| format!("\"{}\"", e))
                .unwrap_or_else(|| "null".to_string()),
            s3_config.access_key,
            s3_config.secret_key,
            version,
            replica_pass
        );
        // Write postgres-backup-config.json via tmp
        interactor.create_file("/tmp/postgres-backup-config.json.tmp", &s3_json_str)?;
        interactor.cmd(
            "sudo mv /tmp/postgres-backup-config.json.tmp /etc/crane/postgres-backup-config.json",
        )?;
        interactor.cmd("sudo chown root:root /etc/crane/postgres-backup-config.json")?;
        interactor.cmd("sudo chmod 600 /etc/crane/postgres-backup-config.json")?;

        // Write postgres-backup.py via tmp
        interactor.create_file("/tmp/postgres-backup.py.tmp", PYTHON_BACKUP_SCRIPT)?;
        interactor.cmd("sudo mv /tmp/postgres-backup.py.tmp /opt/crane/postgres-backup.py")?;
        interactor.cmd("sudo chown root:root /opt/crane/postgres-backup.py")?;
        interactor.cmd("sudo chmod 755 /opt/crane/postgres-backup.py")?;

        // Write cron schedule via tmp
        let full_cron = interval_to_cron(&schedule.full_backup_every);
        let incr_cron = interval_to_cron(&schedule.incremental_backup_every);
        let cron_content = format!(
            r#"
# Crane Postgres Backups
{} root python3 /opt/crane/postgres-backup.py full >> /var/log/crane-backup.log 2>&1
{} root python3 /opt/crane/postgres-backup.py incr >> /var/log/crane-backup.log 2>&1
            "#,
            full_cron, incr_cron
        );
        interactor.create_file("/tmp/postgres-backup-cron.tmp", &cron_content)?;
        interactor.cmd("sudo mv /tmp/postgres-backup-cron.tmp /etc/cron.d/postgres-backup")?;
        interactor.cmd("sudo chown root:root /etc/cron.d/postgres-backup")?;
        interactor.cmd("sudo chmod 644 /etc/cron.d/postgres-backup")?;
    }

    Ok(())
}

pub fn setup_postgres_follower(
    interactor: &dyn ServerInteractor,
    version: &str,
    primary_ip: &str,
    replica_user: &str,
    replica_pass: &str,
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
        if updated_hba != existing_hba.stdout {
            interactor.create_file("/tmp/pg_hba.conf.tmp", &updated_hba)?;
            interactor.cmd(&format!("sudo mv /tmp/pg_hba.conf.tmp '{}'", pg_hba_path))?;
            interactor.cmd(&format!("sudo chown postgres:postgres '{}'", pg_hba_path))?;
            interactor.cmd(&format!("sudo chmod 640 '{}'", pg_hba_path))?;
        }
    }

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
