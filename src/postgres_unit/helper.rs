use crate::config::{self, PostgresBackupSchedule, PostgresDbConfig, PostgresUserConfig};
use crate::helper::cron::interval_to_cron;
use crate::postgres_unit::entity::{BackupMetadata, BackupRegistry};
use crate::postgres_unit::python_backup_script::PYTHON_BACKUP_SCRIPT;
use crate::s3::S3Client;
use crate::server_interactor::get_server_interactor;
use crate::server_interactor::server_interactor_trait::ServerInteractor;
use crate::ssh::SSHSession;

pub fn find_node_config<'a>(
    target: &str,
    config: &'a config::Config,
) -> Option<&'a config::NodeConfig> {
    config.nodes.iter().find(|n| {
        n.host == target || n.internal_ip == target || n.public_ip == target || n.name == target
    })
}

pub fn find_node_config_with_fallback(
    target: &str,
    config: &config::Config,
) -> Option<config::NodeConfig> {
    if let Some(n) = find_node_config(target, config) {
        return Some(n.clone());
    }

    // Fallback: connect to pg nodes and check their hostname
    let pg_nodes: Vec<_> = config
        .nodes
        .iter()
        .filter(|n| n.roles.contains(&"postgres".to_string()))
        .cloned()
        .collect();

    for node in pg_nodes {
        if let Ok(interactor) = connect_to_node(&node, config) {
            if let Ok(h) = interactor.cmd("hostname") {
                if h.stdout.trim() == target {
                    return Some(node);
                }
            }
        }
    }

    None
}

pub fn connect_to_node(
    node: &config::NodeConfig,
    config: &config::Config,
) -> anyhow::Result<Box<dyn ServerInteractor + Send + Sync>> {
    let private_key = crate::helper::keys::find_private_key_for_user(&node.user, config)?;
    let ssh = SSHSession::new(
        node.host.clone(),
        node.user.clone(),
        private_key,
        Some(node.port),
    );

    get_server_interactor(ssh)
}

pub fn get_pg_version(config: &config::Config) -> String {
    config
        .db
        .as_ref()
        .and_then(|db| db.postgres.as_ref())
        .map(|pg| pg.version.clone())
        .unwrap_or_else(|| "16".to_string())
}

pub fn get_replica_pass(config: &config::Config) -> String {
    config
        .db
        .as_ref()
        .and_then(|db| db.postgres.as_ref())
        .map(|pg| pg.replica_pass.clone())
        .unwrap_or_else(|| "repl_password".to_string())
}

pub fn is_postgres_running(interactor: &dyn ServerInteractor, version: &str) -> bool {
    let pg_ctl = format!("/usr/lib/postgresql/{}/bin/pg_ctl", version);
    let status_cmd = format!(
        "sudo -u postgres {} -D /var/lib/postgresql/{}/main status",
        pg_ctl, version
    );
    interactor
        .cmd(&status_cmd)
        .map(|out| out.exit_code == 0)
        .unwrap_or(false)
}

pub fn start_postgres(interactor: &dyn ServerInteractor, version: &str) -> anyhow::Result<()> {
    let pg_ctl = format!("/usr/lib/postgresql/{}/bin/pg_ctl", version);
    let postgres_start_cmd = format!(
        "sudo -u postgres {} -D /var/lib/postgresql/{}/main -o \"-c config_file=/etc/postgresql/{}/main/postgresql.conf -c restore_command=false\" start > /dev/null 2>&1 < /dev/null",
        pg_ctl, version, version
    );

    let out = interactor.cmd(&postgres_start_cmd)?;
    if out.exit_code != 0 {
        println!(
            "error executing postgres_start_cmd {} (exit code: {})",
            postgres_start_cmd, out.exit_code
        );
        println!("\nSTDERR: \n\n{}\n\n", out.stderr);

        anyhow::bail!(
            "Command '{}' failed with exit code {}: {}",
            postgres_start_cmd,
            out.exit_code,
            out.stderr
        );
    }

    Ok(())
}

pub fn ensure_postgres_running(interactor: &dyn ServerInteractor, version: &str) {
    //-> anyhow::Result<()>
    if is_postgres_running(interactor, version) {
        // return Ok(());
        return;
    }

    println!("\tPostgreSQL {} is stopped, starting it...", version);
    let _ = start_postgres(interactor, version);

    for _ in 0..20 {
        if is_postgres_running(interactor, version) {
            // return Ok(());
            return;
        }

        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    eprintln!(
        "Error: PostgreSQL {} is not running and could not be started",
        version
    );
    std::process::exit(1);

    // anyhow::bail!(
    //     "PostgreSQL {} failed to start or respond to status check",
    //     version
    // )
}

pub fn get_postgres_backup_schedule(
    config: &crate::config::Config,
) -> Option<PostgresBackupSchedule> {
    config
        .db
        .as_ref()
        .and_then(|db| db.postgres.as_ref())
        .and_then(|pg| pg.backup.clone())
}

pub fn postgres_get_leader(config: &config::Config) -> anyhow::Result<Option<config::NodeConfig>> {
    let pg_nodes: Vec<_> = config
        .nodes
        .iter()
        .filter(|n| n.roles.contains(&"postgres".to_string()))
        .cloned()
        .collect();

    for node in pg_nodes {
        if let Ok(interactor) = connect_to_node(&node, config) {
            // First check via Patroni REST API
            let curl_patroni_primary_cmd =
                "curl -s -o /dev/null -w \"%{http_code}\" http://localhost:8008/primary";
            let curl_patroni_get_primary = interactor.cmd(curl_patroni_primary_cmd);
            // dbg!(&curl_patroni_get_primary);

            if let Ok(output) = curl_patroni_get_primary {
                if output.stdout.trim() == "200" {
                    return Ok(Some(node));
                }
            }

            // Fallback for mock interactor and compatibility
            let cmd = r#"sudo -u postgres psql -t -A -c "select pg_is_in_recovery();""#;
            if let Ok(output) = interactor.cmd(cmd) {
                if output.stdout.trim() == "f" {
                    return Ok(Some(node));
                }
            }
        }
    }

    Ok(None)
}

pub fn get_backups_from_s3(s3_client: &dyn S3Client) -> anyhow::Result<Vec<BackupMetadata>> {
    let registry_key = "backups/registry.toml";
    match s3_client.get_object(registry_key) {
        Ok(data) => {
            let content = String::from_utf8_lossy(&data).to_string();
            let registry: BackupRegistry = toml::from_str(&content)
                .map_err(|e| anyhow::anyhow!("Failed to parse backups/registry.toml: {}", e))?;
            Ok(registry.backups)
        }

        Err(_) => Ok(Vec::new()),
    }
}

pub fn cmdw(
    interactor: &dyn ServerInteractor,
    command: &str,
) -> anyhow::Result<crate::ssh::CmdOutput> {
    let out = interactor.cmd(command)?;
    if out.exit_code != 0 {
        println!("Command: {}\n", command);
        println!("STDERR:\n{}\n", out.stderr.trim());

        anyhow::bail!(
            "Command '{}' failed with exit code {}: {}",
            command,
            out.exit_code,
            out.stderr.trim()
        );
    }

    Ok(out)
}

pub fn debug_get_postgres_logs(interactor: &dyn ServerInteractor, pg_version: &str) -> String {
    let log_dir = format!("/var/lib/postgresql/{}/main/log", pg_version);
    let find_logs_cmd = format!(
        "sudo find {} -maxdepth 1 -type f \\( -name \"*.log\" -o -name \"*.csv\" \\) -printf \"%T@ %p\\n\" 2>/dev/null | sort -n -r | head -n 5 | cut -d' ' -f2-",
        log_dir
    );

    let mut extra_logs = String::new();
    let mut file_paths = Vec::new();

    if let Ok(find_out) = interactor.cmd(&find_logs_cmd) {
        for line in find_out.stdout.lines() {
            let p = line.trim();
            if !p.is_empty() {
                file_paths.push(p.to_string());
            }
        }
    }

    // Fallback if find output is empty
    if file_paths.is_empty() {
        let fallback_cmd = format!(
            "sudo ls -t {}/*.log {}/*.csv 2>/dev/null | head -n 5",
            log_dir, log_dir
        );
        if let Ok(fb_out) = interactor.cmd(&fallback_cmd) {
            for line in fb_out.stdout.lines() {
                let p = line.trim();
                if !p.is_empty() {
                    file_paths.push(p.to_string());
                }
            }
        }
    }

    for file_path in file_paths {
        extra_logs.push_str(&format!("\n--- Last 50 lines of {} ---\n", file_path));
        let cat_cmd = format!("sudo tail -n 50 '{}'", file_path);
        if let Ok(cat_out) = interactor.cmd(&cat_cmd) {
            extra_logs.push_str(&cat_out.stdout);
        }
    }

    extra_logs
}

fn update_config_value(content: &str, key: &str, value: &str) -> String {
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

pub fn configure_postgresql_conf(
    interactor: &dyn ServerInteractor,
    version: &str,
) -> anyhow::Result<()> {
    let pg_conf_path = format!("/etc/postgresql/{}/main/postgresql.conf", version);
    let existing_conf = interactor.cmd(&format!("sudo cat '{}'", pg_conf_path))?;
    let mut updated_conf = existing_conf.stdout.clone();

    updated_conf = update_config_value(&updated_conf, "listen_addresses", "'*'");
    updated_conf = update_config_value(&updated_conf, "wal_level", "'replica'");
    updated_conf = update_config_value(&updated_conf, "max_wal_senders", "10");
    updated_conf = update_config_value(&updated_conf, "hot_standby", "'on'");
    updated_conf = update_config_value(&updated_conf, "log_statement", "'mod'");
    updated_conf = update_config_value(&updated_conf, "log_min_duration_statement", "0");
    updated_conf = update_config_value(
        &updated_conf,
        "log_line_prefix",
        "'%t [%p]: user=%u db=%d app=%a client=%h '",
    );
    updated_conf = update_config_value(&updated_conf, "log_destination", "'csvlog'");
    updated_conf = update_config_value(&updated_conf, "logging_collector", "'on'");

    // WAL archiving for PITR support
    interactor.cmd("sudo mkdir -p /var/lib/postgresql/wal_archive")?;
    interactor.cmd("sudo chown postgres:postgres /var/lib/postgresql/wal_archive")?;
    interactor.cmd("sudo chmod 700 /var/lib/postgresql/wal_archive")?;
    updated_conf = update_config_value(&updated_conf, "archive_mode", "'on'");
    updated_conf = update_config_value(
        &updated_conf,
        "archive_command",
        "'cp %p /var/lib/postgresql/wal_archive/%f'",
    );

    if version.parse::<i32>().unwrap_or(0) >= 17 {
        updated_conf = update_config_value(&updated_conf, "summarize_wal", "'on'");
    }

    if updated_conf != existing_conf.stdout {
        interactor.create_file(&pg_conf_path, &updated_conf)?;
        interactor.cmd(&format!("sudo chown postgres:postgres '{}'", pg_conf_path))?;
        interactor.cmd(&format!("sudo chmod 644 '{}'", pg_conf_path))?;
    }

    Ok(())
}

pub fn configure_postgres_primary_rules(
    interactor: &dyn ServerInteractor,
    version: &str,
    replica_user: &str,
    follower_ips: &[String],
    app_node_ips: &[String],
) -> anyhow::Result<()> {
    // 1. Configure postgresql.conf parameters directly
    configure_postgresql_conf(interactor, version)?;

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
        interactor.create_file(&pg_hba_path, &updated_hba)?;
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

// Parses database and user configs from TOML structure
pub fn get_postgres_configs(
    config: &crate::config::Config,
) -> (Vec<PostgresDbConfig>, Vec<PostgresUserConfig>) {
    let mut db_configs = Vec::new();
    let mut user_configs = std::collections::HashMap::new();

    if let Some(ref db) = config.db {
        if let Some(ref pg) = db.postgres {
            // 1. Parse databases
            for (_key, val) in &pg.databases {
                db_configs.push(PostgresDbConfig {
                    name: val.name.clone(),
                });
            }

            // 2. config postgres users
            if let Some(ref users_list) = pg.users {
                for u in users_list {
                    let user_entry =
                        user_configs
                            .entry(u.user.clone())
                            .or_insert_with(|| PostgresUserConfig {
                                user: u.user.clone(),
                                password: u.password.clone(),
                                databases: Vec::new(),
                                state: u.state.clone(),
                            });

                    for db_name in &u.databases {
                        if !user_entry.databases.contains(db_name) {
                            user_entry.databases.push(db_name.clone());
                        }
                    }
                }
            }
        }
    }

    let users = user_configs.into_values().collect();

    (db_configs, users)
}

pub fn configure_postgres_backup(
    interactor: &dyn ServerInteractor,
    version: &str,
    replica_pass: &str,
    config: &config::Config,
) -> anyhow::Result<()> {
    if let Some(schedule) = get_postgres_backup_schedule(config) {
        println!("\tSetting up automated cron backups...");

        // Resolve S3Config
        let s3_config = crate::s3::get_s3_config(config)?;

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
        // Write postgres-backup-config.json directly
        interactor.create_file("/etc/crane/postgres-backup-config.json", &s3_json_str)?;
        interactor.cmd("sudo chown root:root /etc/crane/postgres-backup-config.json")?;
        interactor.cmd("sudo chmod 600 /etc/crane/postgres-backup-config.json")?;

        // Write postgres-backup.py directly
        interactor.create_file("/opt/crane/postgres-backup.py", PYTHON_BACKUP_SCRIPT)?;
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
        interactor.create_file("/etc/cron.d/postgres-backup", &cron_content)?;
        interactor.cmd("sudo chown root:root /etc/cron.d/postgres-backup")?;
        interactor.cmd("sudo chmod 644 /etc/cron.d/postgres-backup")?;
    }

    Ok(())
}

pub fn get_postgres_current_timeline_id(
    interactor: &dyn ServerInteractor,
) -> anyhow::Result<String> {
    let db_tli_out = interactor.cmd(
        r#"sudo -u postgres psql -t -A -c "SELECT timeline_id FROM pg_control_checkpoint();""#,
    )?;
    let current_timeline_id = db_tli_out.stdout.trim().to_string();
    Ok(current_timeline_id)
}
