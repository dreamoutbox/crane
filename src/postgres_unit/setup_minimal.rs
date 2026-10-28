use crate::{
    config::{
        self, get_pg_replica_pass, get_postgres_backup_schedule_config,
        get_postgres_dbs_and_users_config,
    },
    helper::config::config_get_nodes,
    postgres_unit::{
        helper::get_pg_version,
        setup_postgres_primary::setup_postgres_primary,
    },
    server_interactor::{get_server_interactor, server_interactor_trait::ServerInteractor},
};

/// Updates or appends a key = value configuration setting in a PostgreSQL configuration file content.
fn update_conf_line(content: &str, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    let mut found = false;

    for line in &mut lines {
        let trimmed = line.trim();
        let mut key_part = trimmed;
        if key_part.starts_with('#') {
            key_part = key_part[1..].trim();
        }
        if key_part.starts_with(key) {
            let rest = key_part[key.len()..].trim();
            if rest.starts_with('=') {
                *line = format!("{} = {}", key, value);
                found = true;
                break;
            }
        }
    }

    if !found {
        lines.push(format!("{} = {}", key, value));
    }

    lines.join("\n") + "\n"
}

/// Discovers the configuration file paths for PostgreSQL based on existing filesystem layout.
fn get_postgres_config_paths(
    interactor: &dyn ServerInteractor,
    pg_version: &str,
) -> (String, String) {
    let debian_conf = format!("/etc/postgresql/{}/main/postgresql.conf", pg_version);
    let debian_hba = format!("/etc/postgresql/{}/main/pg_hba.conf", pg_version);
    if interactor.exists(&debian_conf).unwrap_or(false) {
        return (debian_conf, debian_hba);
    }

    let rhel_conf = format!("/var/lib/pgsql/{}/data/postgresql.conf", pg_version);
    let rhel_hba = format!("/var/lib/pgsql/{}/data/pg_hba.conf", pg_version);
    if interactor.exists(&rhel_conf).unwrap_or(false) {
        return (rhel_conf, rhel_hba);
    }

    (debian_conf, debian_hba)
}

/// Ensures pg_hba.conf permits local connections and authentication for replication and app traffic.
fn configure_pg_hba(
    interactor: &dyn ServerInteractor,
    pg_hba_path: &str,
) -> anyhow::Result<()> {
    let existing_hba = interactor.read_file(pg_hba_path).unwrap_or_default();
    let mut updated_hba = existing_hba.clone();

    if !updated_hba.contains("# Crane minimal mode access rules") {
        if !updated_hba.ends_with('\n') && !updated_hba.is_empty() {
            updated_hba.push('\n');
        }
        updated_hba.push_str("\n# Crane minimal mode access rules\n");
        updated_hba.push_str("local   all             all                                     trust\n");
        updated_hba.push_str("host    all             all             127.0.0.1/32            trust\n");
        updated_hba.push_str("host    all             all             ::1/128                 trust\n");
        updated_hba.push_str("host    all             all             0.0.0.0/0               scram-sha-256\n");
        updated_hba.push_str("host    replication     replicator      127.0.0.1/32            scram-sha-256\n");
        updated_hba.push_str("host    replication     replicator      0.0.0.0/0               scram-sha-256\n");
    }

    if updated_hba != existing_hba {
        interactor.create_file(pg_hba_path, &updated_hba)?;
        let _ = interactor.chown(pg_hba_path, "postgres", "postgres");
        let _ = interactor.chmod(pg_hba_path, "640");
    }

    Ok(())
}

/// Configures core postgresql.conf settings for standalone minimal mode, including WAL archiving.
fn configure_postgresql_conf(
    interactor: &dyn ServerInteractor,
    pg_conf_path: &str,
    pg_version: &str,
    wal_archive_dir: &str,
) -> anyhow::Result<()> {
    let existing_conf = interactor.read_file(pg_conf_path).unwrap_or_default();
    let mut conf = existing_conf.clone();

    conf = update_conf_line(&conf, "listen_addresses", "'*'");
    conf = update_conf_line(&conf, "wal_level", "'replica'");
    conf = update_conf_line(&conf, "max_wal_senders", "10");
    conf = update_conf_line(&conf, "hot_standby", "'on'");
    conf = update_conf_line(&conf, "log_destination", "'csvlog'");
    conf = update_conf_line(&conf, "logging_collector", "'on'");
    conf = update_conf_line(&conf, "archive_mode", "'on'");
    conf = update_conf_line(
        &conf,
        "archive_command",
        &format!("'test ! -f {0}/%f && cp %p {0}/%f'", wal_archive_dir),
    );

    if pg_version.parse::<i32>().unwrap_or(0) >= 17 {
        conf = update_conf_line(&conf, "summarize_wal", "'on'");
    }

    if conf != existing_conf {
        interactor.create_file(pg_conf_path, &conf)?;
        let _ = interactor.chown(pg_conf_path, "postgres", "postgres");
        let _ = interactor.chmod(pg_conf_path, "644");
    }

    Ok(())
}

/// Deploys a standalone PostgreSQL instance on a single node without Patroni or etcd.
///
/// Handles package installation, local service configuration, database and user provisioning,
/// and automated backup scheduling.
///
/// # Arguments
/// * `config` - Loaded system configuration
///
/// # Returns
/// `Ok(())` upon successful deployment and health verification.
///
/// # Example
/// ```no_run
/// use crane::config::Config;
/// use crane::postgres_unit::setup_minimal::setup_postgres_minimal;
///
/// # async fn run(config: &Config) -> anyhow::Result<()> {
/// setup_postgres_minimal(config).await?;
/// # Ok(())
/// # }
/// ```
pub async fn setup_postgres_minimal(config: &config::Config) -> Result<(), anyhow::Error> {
    let pg_version = get_pg_version(config);
    let replica_pass = get_pg_replica_pass(config);

    let schedule = get_postgres_backup_schedule_config(config);
    let s3_config = if schedule.is_some() {
        if crate::config::s3_backup_enabled(config) {
            Some(crate::s3::get_s3_config(config)?)
        } else {
            println!("\tS3 backup is disabled in [backup.s3]. Skipping automated backup cron setup.");
            None
        }
    } else {
        None
    };

    let pg_nodes = config_get_nodes(config, "postgres");
    let node = pg_nodes
        .first()
        .ok_or_else(|| anyhow::anyhow!("No postgres node configured for minimal deployment"))?;

    println!("Setting up minimal standalone PostgreSQL on node {}...", node.name);
    let interactor = get_server_interactor(&node.name)?;

    // 1. Install PostgreSQL binaries
    interactor.install_postgres(&pg_version)?;

    // 2. Ensure Patroni and etcd are stopped and disabled
    let _ = interactor.stop_service("patroni");
    let _ = interactor.disable_service("patroni");
    let _ = interactor.stop_service("etcd");
    let _ = interactor.disable_service("etcd");

    // 3. Configure WAL archive directory
    let wal_archive_dir = interactor.server_paths().pg_wal_archive;
    interactor.mkdir(&wal_archive_dir)?;
    interactor.chown(&wal_archive_dir, "postgres", "postgres")?;
    interactor.chmod(&wal_archive_dir, "700")?;

    // 4. Configure postgresql.conf and pg_hba.conf
    let (conf_path, hba_path) = get_postgres_config_paths(&*interactor, &pg_version);
    configure_postgresql_conf(&*interactor, &conf_path, &pg_version, &wal_archive_dir)?;
    configure_pg_hba(&*interactor, &hba_path)?;

    // 5. Start and enable standard PostgreSQL system service
    println!("\tStarting and enabling PostgreSQL service on node {}...", node.name);
    interactor.service_daemon_reload()?;
    interactor.enable_service("postgresql")?;
    interactor.restart_service("postgresql")?;

    // Poll until PostgreSQL responds to queries
    let mut pg_ready = false;
    let start_time = std::time::Instant::now();
    while start_time.elapsed().as_secs() < 30 {
        if let Ok(res) = interactor.psql(Some("SELECT 1;"), None, None, true) {
            if res.stdout.trim() == "1" {
                pg_ready = true;
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    if !pg_ready {
        anyhow::bail!("Timeout waiting for PostgreSQL service to start on node {}", node.name);
    }
    println!("\tPostgreSQL service is healthy on node {}", node.name);

    // 6. Ensure replicator role exists for backup commands
    let replicator_sql = format!(
        "DO $crane$\n\
         BEGIN\n\
             IF NOT EXISTS (SELECT FROM pg_catalog.pg_roles WHERE rolname = 'replicator') THEN\n\
                 CREATE ROLE replicator WITH REPLICATION LOGIN PASSWORD '{0}';\n\
             ELSE\n\
                 ALTER ROLE replicator WITH REPLICATION PASSWORD '{0}';\n\
             END IF;\n\
         END $crane$;",
        replica_pass
    );
    let tmp_sql = "/tmp/crane_replicator.sql";
    interactor.create_file(tmp_sql, &replicator_sql)?;
    let psql_res = interactor.psql(None, Some(tmp_sql), None, false);
    let _ = interactor.rm(tmp_sql);
    psql_res?;
    let _ = interactor.psql(Some("GRANT pg_read_server_files TO replicator;"), None, None, false);

    // 7. Provision databases and users
    let (db_configs, user_configs) = get_postgres_dbs_and_users_config(config);
    setup_postgres_primary(
        interactor.clone(),
        &pg_version,
        &replica_pass,
        &db_configs,
        &user_configs,
        config,
    )
    .await?;

    // 8. Configure backup cron
    crate::postgres_unit::cron::configure_postgres_cron_backup(
        &*interactor,
        &pg_version,
        &replica_pass,
        &schedule,
        &s3_config,
    )?;

    println!("\tMinimal standalone PostgreSQL setup completed on node {}", node.name);
    Ok(())
}
