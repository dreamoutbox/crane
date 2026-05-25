use crate::{
    config,
    helper::keys::{find_private_key_for_user, get_any_private_key},
    postgres_unit::install::install_postgres,
    server_interactor::server_interactor_trait::ServerInteractor,
    ssh::SSHSession,
};

#[derive(Debug, Clone)]
pub struct PostgresDbConfig {
    pub name: String,
    pub db_name: String,
    pub user: String,
    pub password: Option<String>,
}

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
    Ok(if !pg_nodes.is_empty() {
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
        let db_configs = crate::postgres_unit::setup::get_postgres_db_configs(config);

        // 1. Setup primary node
        println!(
            "Setting up primary PostgreSQL node at {}...",
            primary_node.host
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
        crate::postgres_unit::setup::setup_postgres_primary(
            &*interactor,
            &pg_version,
            "replicator",
            &replica_pass,
            &follower_ips,
            &app_node_ips,
            &db_configs,
        )?;

        // 2. Setup follower nodes
        for follower_node in &pg_nodes[1..] {
            println!(
                "Setting up follower PostgreSQL node at {}...",
                follower_node.host
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
            crate::postgres_unit::setup::setup_postgres_follower(
                &*interactor,
                &pg_version,
                &primary_node.internal_ip,
                "replicator",
                &replica_pass,
            )?;
        }

        // 3. Setup HAProxy on all vps nodes
        for app_node in &app_nodes {
            println!("\tSetting up HAProxy on app node {}...", app_node.host);
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
            crate::postgres_unit::setup::setup_haproxy(
                &*interactor,
                &primary_node.internal_ip,
                &follower_ips,
            )?;
        }
    })
}

pub fn get_postgres_db_configs(config: &crate::config::Config) -> Vec<PostgresDbConfig> {
    let mut db_configs = Vec::new();
    if let Some(ref db) = config.db {
        if let Some(ref pg_map) = db.postgres {
            for (key, val) in pg_map {
                if key == "enabled" || key == "version" {
                    continue;
                }
                if let Some(table) = val.as_table() {
                    let db_name = table
                        .get("db_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let user = table
                        .get("user")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let password = table
                        .get("password")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    if !db_name.is_empty() && !user.is_empty() {
                        db_configs.push(PostgresDbConfig {
                            name: key.clone(),
                            db_name,
                            user,
                            password,
                        });
                    }
                }
            }
        }
    }
    db_configs
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
        "sudo -u postgres {} -D /var/lib/postgresql/{}/main -o \"-c config_file=/etc/postgresql/{}/main/postgresql.conf\" restart > /dev/null 2>&1 < /dev/null",
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
) -> anyhow::Result<()> {
    println!("\tSetting up PostgreSQL primary node...");
    install_postgres_repo_and_pkg(interactor, version)?;

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

    // 5. Idempotently create databases and users
    for db in db_configs {
        println!(
            "Setting up database '{}' and user '{}'...",
            db.db_name, db.user
        );
        let user_sql = format!(
            "DO \\$\\$ BEGIN IF NOT EXISTS (SELECT FROM pg_catalog.pg_roles WHERE rolname = '{}') THEN CREATE ROLE {} WITH PASSWORD '{}' LOGIN; END IF; END \\$\\$;",
            db.user,
            db.user,
            db.password.as_deref().unwrap_or("")
        );
        interactor.cmd(&format!("sudo -u postgres psql -c \"{}\"", user_sql))?;

        let check_db_sql = format!("SELECT 1 FROM pg_database WHERE datname = '{}'", db.db_name);
        let db_exists = interactor.cmd(&format!(
            "sudo -u postgres psql -t -A -c \"{}\"",
            check_db_sql
        ))?;
        if db_exists.stdout.trim() != "1" {
            interactor.cmd(&format!(
                "sudo -u postgres psql -c \"CREATE DATABASE {} OWNER {};\"",
                db.db_name, db.user
            ))?;
        }
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
    install_postgres_repo_and_pkg(interactor, version)?;

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
    println!("\tConfiguring local trust on follower pg_hba.conf...");
    let pg_hba_path = format!("/etc/postgresql/{}/main/pg_hba.conf", version);
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
        "sudo -u postgres {} -D /var/lib/postgresql/{}/main -o \"-c config_file=/etc/postgresql/{}/main/postgresql.conf\" start > /dev/null 2>&1 < /dev/null",
        pg_ctl, version, version
    );
    interactor.cmd(&start_cmd)?;

    Ok(())
}

pub fn setup_haproxy(
    interactor: &dyn ServerInteractor,
    primary_ip: &str,
    follower_ips: &[String],
) -> anyhow::Result<()> {
    println!("\tSetting up HAProxy in front of the PostgreSQL cluster...");

    println!("\tInstalling HAProxy...");
    interactor.install_dependencies(vec!["haproxy".to_string()])?;

    let mut haproxy_cfg = format!(
        r#"
global
    log /dev/log local0
    log /dev/log local1 notice
    chroot /var/lib/haproxy
    user haproxy
    group haproxy
    daemon

defaults
    log global
    mode tcp
    option tcplog
    option dontlognull
    retries 3
    timeout connect 5000ms
    timeout client 50000ms
    timeout server 50000ms

frontend postgres_front
    bind *:5000
    mode tcp
    default_backend postgres_back

backend postgres_back
    mode tcp
    option tcp-check
    server postgres-primary {}:5432 check


"#,
        primary_ip
    );

    for (idx, follower) in follower_ips.iter().enumerate() {
        haproxy_cfg.push_str(&format!(
            "    server postgres-follower-{} {}:5432 check backup\n",
            idx + 1,
            follower
        ));
    }

    println!("\tWriting HAProxy configuration...");
    interactor.create_file("/tmp/haproxy.cfg.tmp", &haproxy_cfg)?;
    interactor.cmd("sudo mv /tmp/haproxy.cfg.tmp /etc/haproxy/haproxy.cfg")?;
    interactor.cmd("sudo chown root:root /etc/haproxy/haproxy.cfg")?;
    interactor.cmd("sudo chmod 644 /etc/haproxy/haproxy.cfg")?;

    println!("\tRestarting and enabling HAProxy service...");
    interactor.cmd("sudo systemctl daemon-reload")?;
    interactor.cmd("sudo systemctl enable haproxy")?;
    interactor.cmd("sudo systemctl restart haproxy")?;

    Ok(())
}
