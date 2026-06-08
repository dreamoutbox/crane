use crate::{
    etcd_unit::etcd_clear_dcs_state,
    helper::config::config_get_nodes,
    postgres_unit::{
        entity::BackupRegistry,
        helper::{
            get_backups_data_from_s3, get_pg_version, pg_cluster_wait_all_nodes_ready,
            pg_get_primary,
        },
    },
    s3::{S3Client, get_s3_config, s3_client::RealS3Client},
    server_interactor::get_server_interactor,
};

pub async fn postgres_restore(
    config: &crate::config::Config,
    target_backup_id: &str,
    base_id: Option<&str>,
    pitr_time: Option<&str>,
) -> anyhow::Result<()> {
    let s3_config = get_s3_config(config)?;

    let primary_node = match pg_get_primary(config)? {
        Some(node) => node,
        None => config
            .nodes
            .iter()
            .find(|n| n.roles.contains(&"postgres".to_string()))
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("No PostgreSQL nodes found in configuration"))?,
    };

    let pg_version = get_pg_version(config);
    let s3_client = RealS3Client::new(&s3_config)?;

    let backups = get_backups_data_from_s3(&s3_client)?;
    let registry = BackupRegistry { backups };

    let mut backup = registry
        .backups
        .iter()
        .find(|b| b.id == target_backup_id)
        .ok_or_else(|| anyhow::anyhow!("Backup ID '{}' not found in registry", target_backup_id))?
        .clone();

    // Validate --base exists in registry if specified
    if let Some(forced_base) = base_id {
        if !registry.backups.iter().any(|b| b.id == forced_base) {
            anyhow::bail!("Base backup ID '{}' not found in registry", forced_base);
        }
    }

    // Build restore chain, stopping at base_id when specified
    let mut chain = Vec::new();
    let mut current = backup.clone();
    chain.push(current.clone());

    while let Some(ref next_base_id) = current.base {
        // If the user specified --base, stop once we've included that backup
        if let Some(forced_base) = base_id {
            if current.id == forced_base {
                break;
            }
        }
        let parent = registry
            .backups
            .iter()
            .find(|b| &b.id == next_base_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Broken backup chain: parent backup ID '{}' not found in registry",
                    next_base_id
                )
            })?;
        chain.push(parent.clone());
        current = parent.clone();
    }

    chain.reverse();

    println!("Backup chain to restore:");
    for item in &chain {
        println!(" - ID: {} ({})", item.id, item.backup_type);
    }

    // Validate --pitr is after the oldest backup in the chain (chain[0] after reverse)
    if let Some(pitr) = pitr_time {
        let pitr_dt = chrono::NaiveDateTime::parse_from_str(pitr, "%Y-%m-%d %H:%M:%S%.f")
            .or_else(|_| chrono::NaiveDateTime::parse_from_str(pitr, "%Y-%m-%d %H:%M:%S"))
            .map_err(|_| {
                anyhow::anyhow!(
                    "--pitr must be in 'YYYY-MM-DD HH:MM:SS' or 'YYYY-MM-DD HH:MM:SS.FFF' format. got `{}`",
                    pitr
                )
            })?;

        let base_backup = &chain[0];
        if let Some(ref taken_at) = base_backup.taken_at {
            let base_dt = chrono::NaiveDateTime::parse_from_str(taken_at, "%Y-%m-%d %H:%M:%S%.f")
                .or_else(|_| chrono::NaiveDateTime::parse_from_str(taken_at, "%Y-%m-%d %H:%M:%S"))
                .map_err(|_| anyhow::anyhow!("Base backup has invalid taken_at: '{}'", taken_at))?;

            if pitr_dt <= base_dt {
                anyhow::bail!(
                    "--pitr time '{}' must be after the base backup time '{}' (backup ID: {})",
                    pitr,
                    taken_at,
                    base_backup.id
                );
            }
        }
    }

    let interactor_arc = get_server_interactor(&primary_node.name)?;
    let interactor = &*interactor_arc;

    println!("Restoring database to backup ID: {}\n", target_backup_id);
    if let Some(t) = pitr_time {
        println!("Point-in-time recovery target: {}", t);
    }

    if let Some(pitr) = pitr_time {
        let pitr_dt = chrono::NaiveDateTime::parse_from_str(pitr, "%Y-%m-%d %H:%M:%S%.f")
            .or_else(|_| chrono::NaiveDateTime::parse_from_str(pitr, "%Y-%m-%d %H:%M:%S"))
            .map_err(|_| {
                anyhow::anyhow!(
                    "--pitr must be in 'YYYY-MM-DD HH:MM:SS' or 'YYYY-MM-DD HH:MM:SS.FFF' format. got `{}`",
                    pitr
                )
            })?;

        let mut filtered_chain = Vec::new();
        for item in &chain {
            if let Some(ref taken_at) = item.taken_at {
                let backup_dt =
                    chrono::NaiveDateTime::parse_from_str(taken_at, "%Y-%m-%d %H:%M:%S%.f")
                        .or_else(|_| {
                            chrono::NaiveDateTime::parse_from_str(taken_at, "%Y-%m-%d %H:%M:%S")
                        })
                        .map_err(|_| {
                            anyhow::anyhow!("Backup has invalid taken_at: '{}'", taken_at)
                        })?;

                if backup_dt < pitr_dt {
                    filtered_chain.push(item.clone());
                }
            } else {
                filtered_chain.push(item.clone());
            }
        }

        if filtered_chain.is_empty() {
            anyhow::bail!(
                "No backup in the chain starts before the PITR target time '{}'",
                pitr
            );
        }

        chain = filtered_chain;
        backup = chain.last().unwrap().clone();
    }

    let server_paths = interactor.server_paths();
    // let pg_ctl = interactor.pg_bin_path(&pg_version, "pg_ctl");

    let pg_combinebackup = interactor.pg_bin_path(&pg_version, "pg_combinebackup");
    let pg_verifybackup = interactor.pg_bin_path(&pg_version, "pg_verifybackup");
    let pgdata_dir = format!("{}/{}/main", server_paths.pg_data_dir, pg_version);

    // Gather all PostgreSQL nodes
    let pg_nodes = config_get_nodes(&config, "postgres");

    // 1. Stop all Patroni and PostgreSQL on all nodes
    let mut handles = vec![];
    for node in &pg_nodes {
        let node = node.clone();

        let handle = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            println!(
                "Stopping Patroni/PostgreSQL on node {} for restore",
                node.name
            );

            match get_server_interactor(&node.name) {
                Ok(interactor) => {
                    let _ = interactor.stop_service("patroni");
                    let _ = interactor.stop_service("postgresql --no-block");
                    let _ = interactor.kill_postgres_processes();
                }
                Err(e) => {
                    println!("Warning: failed to connect to node {}: {}", node.name, e);
                }
            }
            Ok(())
        });

        handles.push(handle);
    }
    for handle in handles {
        handle.await??;
    }

    // 2. Clear DCS (etcd) keys for the cluster to prevent conflicts
    println!("\nClearing Etcd DCS cluster state...");
    etcd_clear_dcs_state(interactor);

    // 3. Clear existing data directory on all nodes
    let mut handles = vec![];
    for node in &pg_nodes {
        let node = node.clone();
        let pg_version = pg_version.clone();

        let handle = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            // println!("\tClearing postgres data directory on node {}", node.name);

            match get_server_interactor(&node.name) {
                Ok(interactor) => {
                    let node_paths = interactor.server_paths();
                    let node_pgdata_dir = format!("{}/{}/main", node_paths.pg_data_dir, pg_version);
                    let _ = interactor.rm(&node_pgdata_dir);
                    interactor.mkdir(&node_pgdata_dir)?;
                    interactor.chown(&node_pgdata_dir, "postgres", "postgres")?;
                    interactor.chmod(&node_pgdata_dir, "700")?;
                    println!("\tCleared postgres data on node {}", node.name);
                }
                Err(e) => {
                    println!("\tWarning: failed to connect to node {}: {}", node.name, e);
                }
            }
            Ok(())
        });

        handles.push(handle);
    }
    for handle in handles {
        handle.await??;
    }

    // 2. Download all backups in the chain from S3 to VPS local backups dir
    interactor.mkdir(&server_paths.pg_backup_dir)?;
    interactor.chown(&server_paths.pg_backup_dir, "postgres", "postgres")?;
    interactor.chmod(&server_paths.pg_backup_dir, "755")?;

    for item in &chain {
        let remote_dir = format!("{}/{}", server_paths.pg_backup_dir, item.id);
        interactor.mkdir(&remote_dir)?;
        interactor.chown(&remote_dir, "postgres", "postgres")?;
        interactor.chmod(&remote_dir, "755")?;

        // base.tar and backup_manifest are required; pg_wal.tar is optional
        let required_files = ["base.tar", "backup_manifest"];
        for file in required_files {
            let s3_key = format!("backups/{}/{}", item.id, file);

            let data = s3_client.get_object(&s3_key).map_err(|_| {
                anyhow::anyhow!(
                    "Backup '{}' is incomplete: required file '{}' not found in S3. \
                     This backup cannot be restored.",
                    item.id,
                    file
                )
            })?;

            let temp_path =
                std::env::temp_dir().join(format!("crane-restore-{}-{}", item.id, file));
            std::fs::write(&temp_path, &data)?;

            let remote_temp_file = format!("/tmp/crane-restore-{}-{}", item.id, file);
            interactor.upload(temp_path.to_str().unwrap(), &remote_temp_file)?;
            let _ = std::fs::remove_file(temp_path);

            let remote_file = format!("{}/{}", remote_dir, file);
            interactor.mv(&remote_temp_file, &remote_file)?;
            interactor.chown(&remote_file, "postgres", "postgres")?;
            interactor.chmod(&remote_file, "644")?;
        }

        // pg_wal.tar is optional
        let wal_s3_key = format!("backups/{}/pg_wal.tar", item.id);
        if let Ok(data) = s3_client.get_object(&wal_s3_key) {
            let temp_path =
                std::env::temp_dir().join(format!("crane-restore-{}-pg_wal.tar", item.id));
            std::fs::write(&temp_path, &data)?;

            let remote_temp_file = format!("/tmp/crane-restore-{}-pg_wal.tar", item.id);
            interactor.upload(temp_path.to_str().unwrap(), &remote_temp_file)?;
            let _ = std::fs::remove_file(temp_path);

            let remote_file = format!("{}/pg_wal.tar", remote_dir);
            interactor.mv(&remote_temp_file, &remote_file)?;
            interactor.chown(&remote_file, "postgres", "postgres")?;
            interactor.chmod(&remote_file, "644")?;
        }
    }

    if chain.len() <= 1 {
        // 3. Clear data directory
        interactor.rm(&pgdata_dir)?;
        interactor.mkdir(&pgdata_dir)?;
        interactor.chown(&pgdata_dir, "postgres", "postgres")?;
        interactor.chmod(&pgdata_dir, "700")?;

        // 4. Extract base.tar
        let base_tar_path = format!("{}/{}/base.tar", server_paths.pg_backup_dir, backup.id);
        interactor.tar_extract(&base_tar_path, &pgdata_dir)?;
        interactor.chown(&pgdata_dir, "postgres", "postgres")?;

        // 5. Extract pg_wal.tar if present
        let wal_path = format!("{}/{}/pg_wal.tar", server_paths.pg_backup_dir, backup.id);
        if interactor.exists(&wal_path)? {
            let pg_wal_dir = format!("{}/pg_wal", pgdata_dir);
            interactor.mkdir(&pg_wal_dir)?;
            interactor.chown(&pg_wal_dir, "postgres", "postgres")?;

            interactor.tar_extract(&wal_path, &pg_wal_dir)?;
            interactor.chown(&pg_wal_dir, "postgres", "postgres")?;
        }
    } else {
        // 3. Extract all backups in the chain to separate folders
        for item in &chain {
            let extracted_dir = format!("{}/{}_extracted", server_paths.pg_backup_dir, item.id);
            interactor.rm(&extracted_dir)?;
            interactor.mkdir(&extracted_dir)?;
            interactor.chown(&extracted_dir, "postgres", "postgres")?;
            let base_tar_path = format!("{}/{}/base.tar", server_paths.pg_backup_dir, item.id);
            interactor.tar_extract(&base_tar_path, &extracted_dir)?;
            interactor.chown(&extracted_dir, "postgres", "postgres")?;

            // Copy backup_manifest to extracted directory so pg_combinebackup can find it
            let manifest_src =
                format!("{}/{}/backup_manifest", server_paths.pg_backup_dir, item.id);
            interactor.cp(&manifest_src, &extracted_dir)?;
            let manifest_dest = format!("{}/backup_manifest", extracted_dir);
            interactor.chown(&manifest_dest, "postgres", "postgres")?;
            interactor.chmod(&manifest_dest, "644")?;
        }

        // 4. Combine backups
        let combined_dir_str = format!("{}/combined", server_paths.pg_backup_dir);
        let combined_dir = &combined_dir_str;
        interactor.rm(combined_dir)?;

        let mut combine_cmd = format!("sudo -u postgres {} ", pg_combinebackup);
        for item in &chain {
            combine_cmd.push_str(&format!(
                "{}/{}_extracted ",
                server_paths.pg_backup_dir, item.id
            ));
        }
        combine_cmd.push_str(&format!("-o {}", combined_dir));
        interactor.cmd(&combine_cmd)?;

        // Extract target backup's pg_wal.tar to combined_dir/pg_wal if present
        let wal_path = format!("{}/{}/pg_wal.tar", server_paths.pg_backup_dir, backup.id);
        if interactor.exists(&wal_path)? {
            let combined_wal_dir = format!("{}/pg_wal", combined_dir);
            interactor.mkdir(&combined_wal_dir)?;
            interactor.chown(&combined_wal_dir, "postgres", "postgres")?;

            interactor.tar_extract(&wal_path, &combined_wal_dir)?;
            interactor.chown(&combined_wal_dir, "postgres", "postgres")?;
        }

        // 5. Verify the combined backup
        let verify_cmd = format!("sudo -u postgres {} {}", pg_verifybackup, combined_dir);
        interactor.cmd(&verify_cmd)?;

        // 6. Clear and replace data directory with combined backup
        interactor.rm(&pgdata_dir)?;
        interactor.mv(combined_dir, &pgdata_dir)?;

        // Clean up extracted directories
        for item in &chain {
            let extracted_dir = format!("{}/{}_extracted", server_paths.pg_backup_dir, item.id);
            let _ = interactor.rm(&extracted_dir);
        }
    }

    // 6. Set ownership
    interactor.chown(&pgdata_dir, "postgres", "postgres")?;
    interactor.chmod(&pgdata_dir, "700")?;

    // Remove old signals and dynamic JSON on primary node
    let _ = interactor.cmd(&format!(
        "sudo -u postgres rm -f {}/recovery.signal {}/standby.signal {}/patroni.dynamic.json",
        pgdata_dir, pgdata_dir, pgdata_dir
    ));

    let pg_ctl = interactor.pg_bin_path(&pg_version, "pg_ctl");

    if let Some(target_time) = pitr_time {
        // Write PITR settings to postgresql.auto.conf in pgdata_dir
        let pitr_conf_path = format!("{}/postgresql.auto.conf", pgdata_dir);
        let mut current_conf = interactor.read_file(&pitr_conf_path).unwrap_or_default();
        let pitr_conf_content = format!(
            "restore_command = 'cp {}/wal_archive/%f %p'\nrecovery_target_time = '{}'\nrecovery_target_action = promote\nrecovery_target_inclusive = on\nrecovery_target_timeline = 'current'\n",
            server_paths.pg_data_dir, target_time
        );
        if !current_conf.is_empty() && !current_conf.ends_with('\n') {
            current_conf.push('\n');
        }
        current_conf.push_str(&pitr_conf_content);
        interactor.create_file(&pitr_conf_path, &current_conf)?;
        interactor.chown(&pitr_conf_path, "postgres", "postgres")?;

        // Create recovery.signal (PG12+ triggers archive recovery mode)
        let recovery_signal_path = format!("{}/recovery.signal", pgdata_dir);
        interactor.create_file(&recovery_signal_path, "")?;
        interactor.chown(&recovery_signal_path, "postgres", "postgres")?;

        // Start PostgreSQL directly using pg_ctl to perform the archive recovery (PITR).
        // This prevents Patroni from overwriting our recovery configuration on startup.
        // We use -l /tmp/pg_start.log to prevent open stdout/stderr from hanging the SSH session.
        println!("Performing Point-in-Time Recovery via direct PostgreSQL start...");
        let pg_start_out = interactor.cmd(&format!(
            "sudo -u postgres {} -D {} -l /tmp/pg_start.log start",
            pg_ctl, pgdata_dir
        ))?;
        if pg_start_out.exit_code != 0 {
            if let Ok(pg_start_log_out) = interactor.cmd("sudo -u postgres cat /tmp/pg_start.log") {
                println!("--- PostgreSQL LOG DUMP (/tmp/pg_start.log) ---");
                println!("{}", pg_start_log_out.stdout);
                println!("{}", pg_start_log_out.stderr);
                println!("\n------------------------------------------------\n");
            }

            if let Ok(pg_log_out) = interactor.cmd(&format!(
                "sudo -u postgres cat {}/log/*.log {}/log/*.csv",
                pgdata_dir, pgdata_dir
            )) {
                println!("--- PostgreSQL LOG DUMP ({}/log) ---", pgdata_dir);
                println!("{}", pg_log_out.stdout);
                println!("{}", pg_log_out.stderr);
                println!("\n------------------------------------------------\n");
            }

            anyhow::bail!(
                "Failed to start PostgreSQL directly for PITR: {}",
                pg_start_out.stderr
            );
        }

        // Poll pg_is_in_recovery() until it returns 'f' (meaning it has reached the recovery target and promoted).
        let start_time = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(60);
        let mut recovery_complete = false;

        while start_time.elapsed() < timeout {
            let check_res = interactor.psql(Some("SELECT pg_is_in_recovery();"), None, None, true);
            if let Ok(out) = check_res {
                if out.stdout.trim() == "f" {
                    recovery_complete = true;
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }

        // Stop PostgreSQL directly
        println!("Stopping direct PostgreSQL instance...");
        let _ = interactor.cmd(&format!(
            "sudo -u postgres {} -D {} stop -m fast",
            pg_ctl, pgdata_dir
        ));
        let _ = interactor.rm("/tmp/pg_start.log");

        if !recovery_complete {
            anyhow::bail!(
                "Timeout waiting for Point-in-Time Recovery to complete and promote database."
            );
        }
        println!("PITR recovery complete and database promoted!");
    }

    // Start Patroni on the primary node only
    println!("Starting Patroni on primary node {}...", primary_node.name);
    interactor.restart_service("patroni")?;

    // Wait for primary node to become the Patroni leader
    println!("Waiting a node to become Patroni leader...");
    let mut primary_ready = false;
    let start_time = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(300); // 5 minutes

    while start_time.elapsed() < timeout {
        if let Ok(code) = interactor.check_http_status("http://127.0.0.1:8008/primary") {
            if code == 200 {
                primary_ready = true;
                break;
            }
        }

        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    if !primary_ready {
        let logs = interactor
            .cmd("sudo journalctl -u patroni -n 100 --no-pager")
            .map(|o| o.stdout)
            .unwrap_or_default();

        println!("\n\nPatroni LOGS DUMP: {logs}\n\n");

        anyhow::bail!("Timeout waiting for primary node to become Patroni leader.",);
    }

    // println!(
    //     "Primary node {} is now the Patroni leader.",
    //     primary_node.name
    // );

    if pitr_time.is_some() {
        // Clean up PITR settings from postgresql.auto.conf on primary
        let pitr_conf_path = format!("{}/postgresql.auto.conf", pgdata_dir);
        let _ = interactor.cmd(&format!(
            "sudo -u postgres sed -i '/restore_command/d;/recovery_target/d' {}",
            pitr_conf_path
        ));
        let _ = interactor.psql(Some("SELECT pg_reload_conf();"), None, None, false);
    }

    // Start Patroni on replica nodes
    let mut handles = vec![];
    for node in &pg_nodes {
        if node.internal_ip == primary_node.internal_ip {
            continue;
        }
        let node = node.clone();

        let handle = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            println!("Starting Patroni on replica node {}...", node.name);
            let node_interactor = get_server_interactor(&node.name)?;
            node_interactor.restart_service("patroni")?;
            Ok(())
        });

        handles.push(handle);
    }
    for handle in handles {
        handle.await??;
    }

    // Wait for all replica nodes to join and reach "running" state
    println!("Waiting for all nodes to join the cluster...");
    pg_cluster_wait_all_nodes_ready(interactor, &pg_nodes);

    Ok(())
}
