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
            let is_patroni_primary =
                if let Ok(code) = interactor.check_http_status("http://127.0.0.1:8008/primary") {
                    code == 200
                } else {
                    false
                };

            if is_patroni_primary {
                // Verify it's actually writable (out of recovery)
                if let Ok(output) =
                    interactor.psql(Some("select pg_is_in_recovery();"), None, None, true)
                {
                    // dbg!(&output);

                    if output.stdout.trim() == "f" {
                        return Ok(Some(node));
                    }
                }
            } else {
                // Fallback for mock interactor and compatibility
                if let Ok(output) =
                    interactor.psql(Some("select pg_is_in_recovery();"), None, None, true)
                {
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

        let list_cmd = format!(
            "sudo -u postgres patronictl -c {} list",
            interactor.server_paths().patroni_config_path
        );

        while replica_start_time.elapsed() < replica_timeout {
            if let Ok(out) = interactor.cmd(&list_cmd) {
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

    let paths = interactor.server_paths();
    // pg_data_dir is e.g. /var/lib/postgresql (Debian) or /var/lib/pgsql (RHEL)
    // full data dir is {pg_data_dir}/{version}/{subdir} where subdir is "main" or "data"
    let pg_data_base = format!("{}/{}", paths.pg_data_dir, pg_version);

    // Mirror the real data dir under /backup/{timestamp}/
    let backup_parent = format!(
        "/backup/{}{}/{}",
        unix_timestamp, paths.pg_data_dir, pg_version
    );

    // Each interactor uses its own subdir name (Debian: "main", RHEL: "data")
    for subdir in &["main", "data"] {
        let old_dir = format!("{}/{}", pg_data_base, subdir);
        let backup_dir = format!("{}/{}", backup_parent, subdir);
        if interactor.exists(&old_dir).unwrap_or(false) {
            println!(
                "\tBacking up old postgres data directory {} to {}",
                old_dir, backup_dir
            );
            interactor.mkdir(&backup_parent)?;
            interactor.mv(&old_dir, &backup_dir)?;
        }

        let failed_dir = format!("{}/{}.failed", pg_data_base, subdir);
        let backup_failed_dir = format!("{}/{}.failed", backup_parent, subdir);
        if interactor.exists(&failed_dir).unwrap_or(false) {
            println!(
                "\tBacking up failed data directory {} to {}...",
                failed_dir, backup_failed_dir
            );
            interactor.mkdir(&backup_parent)?;
            interactor.mv(&failed_dir, &backup_failed_dir)?;
        }
    }

    Ok(())
}
