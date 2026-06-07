use crate::config;
use crate::helper::config::config_get_nodes;

use crate::postgres_unit::entity::{BackupMetadata, BackupRegistry};
use crate::s3::S3Client;
use crate::server_interactor::get_server_interactor;
use crate::server_interactor::server_interactor_trait::ServerInteractor;

pub fn get_pg_version(config: &config::Config) -> String {
    config
        .db
        .as_ref()
        .and_then(|db| db.postgres.as_ref())
        .map(|pg| pg.version.clone())
        .unwrap_or_else(|| "17".to_string())
}

pub fn pg_get_primary(config: &config::Config) -> anyhow::Result<Option<config::NodeConfig>> {
    let pg_nodes = config_get_nodes(config, "postgres");

    for node in pg_nodes {
        if let Ok(interactor) = get_server_interactor(&node.name) {
            // First check via Patroni REST API
            let is_patroni_primary = if let Ok(code) = interactor.check_http_status("http://127.0.0.1:8008/primary") {
                code == 200
            } else {
                false
            };

            if is_patroni_primary {
                // Verify it's actually writable (out of recovery)
                if let Ok(output) = interactor.psql(Some("select pg_is_in_recovery();"), None, None, true) {
                    // dbg!(&output);

                    if output.stdout.trim() == "f" {
                        return Ok(Some(node));
                    }
                }
            } else {
                // Fallback for mock interactor and compatibility
                if let Ok(output) = interactor.psql(Some("select pg_is_in_recovery();"), None, None, true) {
                    if output.stdout.trim() == "f" {
                        return Ok(Some(node));
                    }
                }
            }
        }
    }

    Ok(None)
}

pub fn get_backups_data_from_s3(s3_client: &dyn S3Client) -> anyhow::Result<Vec<BackupMetadata>> {
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

/// get current timeline id
pub fn get_pg_current_timeline_id(interactor: &dyn ServerInteractor) -> anyhow::Result<String> {
    let db_tli_out = interactor.psql(
        Some("SELECT timeline_id FROM pg_control_checkpoint();"),
        None,
        None,
        true,
    )?;
    let current_timeline_id = db_tli_out.stdout.trim().to_string();
    Ok(current_timeline_id)
}

pub fn pg_cluster_wait_all_nodes_ready(
    interactor: &dyn ServerInteractor,
    pg_nodes: &Vec<crate::config::NodeConfig>,
) -> bool {
    if pg_nodes.len() > 1 {
        let replica_start_time = std::time::Instant::now();
        let replica_timeout = std::time::Duration::from_secs(90);

        let list_cmd = "sudo -u postgres patronictl -c /etc/patroni/config.yml list";

        while replica_start_time.elapsed() < replica_timeout {
            if let Ok(out) = interactor.cmd(list_cmd) {
                let output = out.stdout;
                let mut all_running = true;

                for node in pg_nodes {
                    let node_line = output.lines().find(|l| l.contains(&node.name));
                    // dbg!(&node_line);

                    match node_line {
                        Some(line) => {
                            // if not `streaming` or `running` it means that the node is not healthy
                            if !line.contains("streaming") && !line.contains("running") {
                                all_running = false;
                            }
                        }

                        None => {
                            all_running = false;
                        }
                    }
                }

                if all_running {
                    return true;
                }
            }

            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        false
    } else {
        true
    }
}

pub fn backup_pg_dir(
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
    let dir_exists = interactor.exists(&old_main_dir).unwrap_or(false);
    if dir_exists {
        println!(
            "\tBacking up old postgres data directory {} to {}",
            old_main_dir, backup_main_dir
        );
        interactor.mkdir(&backup_parent)?;
        interactor.mv(&old_main_dir, &backup_main_dir)?;
    }

    let failed_main_dir = format!("/var/lib/postgresql/{}/main.failed", *pg_version);
    let backup_failed_dir = format!("{}/main.failed", backup_parent);
    let failed_exists = interactor.exists(&failed_main_dir).unwrap_or(false);
    if failed_exists {
        println!(
            "\tBacking up failed data directory {} to {}...",
            failed_main_dir, backup_failed_dir
        );
        interactor.mkdir(&backup_parent)?;
        interactor.mv(&failed_main_dir, &backup_failed_dir)?;
    }

    Ok(())
}
