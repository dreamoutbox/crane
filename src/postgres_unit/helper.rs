use crate::config::{self, PostgresBackupSchedule};
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
) -> anyhow::Result<Box<dyn ServerInteractor>> {
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
        .and_then(|pg| pg.get("version"))
        .and_then(|val| val.as_str())
        .unwrap_or("16")
        .to_string()
}

pub fn get_replica_pass(dot_env: &std::collections::HashMap<String, String>) -> String {
    dot_env
        .get("POSTGRES_PASSWORD")
        .cloned()
        .unwrap_or_else(|| "repl_password".to_string())
}

pub fn is_postgres_running(interactor: &Box<dyn ServerInteractor>, version: &str) -> bool {
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

pub fn start_postgres(interactor: &Box<dyn ServerInteractor>, version: &str) -> anyhow::Result<()> {
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

pub fn ensure_postgres_running(interactor: &Box<dyn ServerInteractor>, version: &str) {
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
    if let Some(ref db) = config.db {
        if let Some(ref pg_map) = db.postgres {
            if let Some(backup_val) = pg_map.get("backup") {
                if let Some(backup_table) = backup_val.as_table() {
                    let full = backup_table
                        .get("full_backup_every")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    let incremental = backup_table
                        .get("incremental_backup_every")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    if !full.is_empty() && !incremental.is_empty() {
                        return Some(PostgresBackupSchedule {
                            full_backup_every: full,
                            incremental_backup_every: incremental,
                        });
                    }
                }
            }
        }
    }
    None
}

pub fn interval_to_cron(interval: &str) -> String {
    let num_str: String = interval.chars().filter(|c| c.is_ascii_digit()).collect();
    let unit: String = interval.chars().filter(|c| c.is_alphabetic()).collect();
    let num: u32 = num_str.parse().unwrap_or(1);

    match unit.as_str() {
        "m" => {
            if num == 1 {
                "* * * * *".to_string()
            } else {
                format!("*/{} * * * *", num)
            }
        }
        "h" => {
            if num == 1 {
                "0 * * * *".to_string()
            } else {
                format!("0 */{} * * *", num)
            }
        }
        "d" => {
            if num == 1 {
                "0 0 * * *".to_string()
            } else {
                format!("0 0 */{} * *", num)
            }
        }
        _ => "0 0 * * *".to_string(), // default to daily
    }
}
