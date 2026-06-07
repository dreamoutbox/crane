use crate::{
    postgres_unit::{
        entity::{BackupMetadata, BackupRegistry},
        helper::{get_backups_data_from_s3, get_pg_current_timeline_id},
    },
    s3::S3Client,
    server_interactor::server_interactor_trait::ServerInteractor,
};

pub fn postgres_backup(
    interactor: &dyn ServerInteractor,
    s3_client: &dyn S3Client,
    pg_version: &str,
    backup_type: &str,
    replica_pass: &str,
    bucket_name: &str,
    last_backup: Option<&BackupMetadata>,
    label: Option<&str>,
) -> anyhow::Result<BackupMetadata> {
    // 1. Get Date and Time from DB Node
    let date_output = interactor.cmd("date +'%Y%m%d%H%M%S%3N %Y-%m-%d %H:%M:%S.%3N'")?;
    let parts: Vec<&str> = date_output.stdout.trim().split_whitespace().collect();
    if parts.len() < 3 {
        anyhow::bail!(
            "Failed to parse date output from server: '{}'",
            date_output.stdout
        );
    }

    let id = parts[0].to_string();
    let date = parts[1].to_string();
    let time = parts[2].to_string();

    let local_path = format!("/var/lib/postgresql/backups/{}", id);
    let pg_basebackup = format!("/usr/lib/postgresql/{}/bin/pg_basebackup", pg_version);
    let pg_verifybackup = format!("/usr/lib/postgresql/{}/bin/pg_verifybackup", pg_version);

    // 2. Ensure Backup Directories exist
    interactor.mkdir("/var/lib/postgresql/backups")?;
    interactor.chown("/var/lib/postgresql/backups", "postgres", "postgres")?;
    interactor.chmod("/var/lib/postgresql/backups", "755")?;
    interactor.mkdir(&local_path)?;
    interactor.chown(&local_path, "postgres", "postgres")?;

    // 3. Grant pg_read_server_files to replicator (idempotent)
    interactor.psql(
        Some("GRANT pg_read_server_files TO replicator;"),
        None,
        None,
        false,
    )?;

    // 4. Build pg_basebackup command
    let is_incr =
        backup_type.to_lowercase() == "incr" || backup_type.to_lowercase() == "incremental";
    let mut pgbasebackup_cmd = format!(
        "sudo -u postgres PGPASSWORD='{}' {} -h localhost -U replicator -D {} -F t -X s -c fast --manifest-checksums=sha256",
        replica_pass, pg_basebackup, local_path
    );

    let mut base_id = None;
    if is_incr {
        if let Some(parent) = last_backup {
            base_id = Some(parent.id.clone());
            let parent_manifest =
                format!("/var/lib/postgresql/backups/{}/backup_manifest", parent.id);

            // Check if parent manifest is present locally
            if !interactor.exists(&parent_manifest)? {
                // Recreate parent directory and restore manifest from S3
                let parent_dir = format!("/var/lib/postgresql/backups/{}", parent.id);
                interactor.mkdir(&parent_dir)?;
                interactor.chown(&parent_dir, "postgres", "postgres")?;
                interactor.chmod(&parent_dir, "755")?;
                let s3_key = format!("backups/{}/backup_manifest", parent.id);
                let manifest_data = s3_client.get_object(&s3_key)?;

                // Write back on VPS
                let content = String::from_utf8_lossy(&manifest_data);
                interactor.create_file(&parent_manifest, &content)?;
                interactor.chown(&parent_manifest, "postgres", "postgres")?;
                interactor.chmod(&parent_manifest, "644")?;
            }

            // Check parent's timeline vs current database's timeline
            let parent_tli_out = interactor.cmd(&format!(
                "sudo -u postgres python3 -c \"import json; m=json.load(open('{}')); print(next(iter(m.get('WAL-Ranges', [])), {{}}).get('Timeline', 0))\"",
                parent_manifest
            ))?;
            let parent_timeline_id = parent_tli_out.stdout.trim();

            let current_timeline_id = get_pg_current_timeline_id(interactor)?;

            if !parent_timeline_id.is_empty()
                && !current_timeline_id.is_empty()
                && parent_timeline_id != current_timeline_id
            {
                let _ = interactor.rm(&local_path);
                anyhow::bail!(
                    "Timeline mismatch detected: parent backup timeline is {}, but current database timeline is {}. consider full backup first!",
                    parent_timeline_id,
                    current_timeline_id
                );
            } else {
                pgbasebackup_cmd.push_str(&format!(" --incremental={}", parent_manifest));
            }
        } else {
            anyhow::bail!("Cannot perform incremental backup: no previous backup found.");
        }
    }

    if is_incr && pg_version.parse::<i32>().unwrap_or(0) >= 17 {
        // Get the current WAL LSN before switching, to use as the synchronization target.
        let lsn_out = interactor.psql(Some("SELECT pg_current_wal_lsn();"), None, None, true)?;
        let target_lsn = lsn_out.stdout.trim().to_string();

        // Force a WAL switch so the active segment is closed for summarization, and CHECKPOINT to flush summaries.
        let _ = interactor.psql(
            Some("SELECT pg_switch_wal(); CHECKPOINT;"),
            None,
            None,
            false,
        );

        // Poll WAL summarizer until summarized_lsn >= target_lsn.
        let max_retries = 30;
        let query = format!(
            "SELECT summarized_lsn >= '{}'::pg_lsn FROM pg_get_wal_summarizer_state();",
            target_lsn
        );
        for attempt in 1..=max_retries {
            let state = interactor.psql(Some(&query), None, None, true)?;
            let output = state.stdout.trim();
            if output == "t" {
                println!(
                    "WAL summarizer caught up to target LSN {} (attempt {})",
                    target_lsn, attempt
                );
                break;
            }

            if attempt == max_retries {
                // Get current full state for debugging info on failure
                let debug_state = interactor.psql(
                    Some("SELECT summarized_lsn, pending_lsn FROM pg_get_wal_summarizer_state();"),
                    None,
                    None,
                    true,
                )?;

                anyhow::bail!(
                    "WAL summarizer did not catch up to target {} after {}s. State: {}",
                    target_lsn,
                    max_retries,
                    debug_state.stdout.trim()
                );
            }

            println!(
                "Waiting for WAL summarizer to reach {}... (attempt {}/{})",
                target_lsn, attempt, max_retries
            );

            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }

    // 5. Run pg_basebackup
    println!("\nRunning pg_basebackup command: {}\n", pgbasebackup_cmd);
    let pgbasebackup_out = interactor.cmd(&pgbasebackup_cmd)?;
    if pgbasebackup_out.exit_code != 0 {
        let _ = interactor.rm(&local_path);
        anyhow::bail!(
            "pg_basebackup failed with exit code {}: {}",
            pgbasebackup_out.exit_code,
            pgbasebackup_out.stderr.trim()
        );
    }

    // 6. Verify Backup (requires extracting tar to a temp directory first)
    let verify_dir = format!("/var/lib/postgresql/backups/{}_verify", id);
    interactor.mkdir(&verify_dir)?;
    interactor.chown(&verify_dir, "postgres", "postgres")?;
    interactor.tar_extract(&format!("{}/base.tar", local_path), &verify_dir)?;
    interactor.chown(&verify_dir, "postgres", "postgres")?;

    // Check if pg_wal.tar exists and extract it
    let wal_path = format!("{}/pg_wal.tar", local_path);
    if interactor.exists(&wal_path)? {
        let verify_wal_dir = format!("{}/pg_wal", verify_dir);
        interactor.mkdir(&verify_wal_dir)?;
        interactor.chown(&verify_wal_dir, "postgres", "postgres")?;
        interactor.tar_extract(&format!("{}/pg_wal.tar", local_path), &verify_wal_dir)?;
        interactor.chown(&verify_wal_dir, "postgres", "postgres")?;
    }

    // Copy backup_manifest to verify_dir
    let manifest_path = format!("{}/backup_manifest", local_path);
    interactor.cp(&manifest_path, &verify_dir)?;
    interactor.chown(&verify_dir, "postgres", "postgres")?;

    let verify_cmd = format!("sudo -u postgres {} {}", pg_verifybackup, verify_dir);
    println!("Running pg_verifybackup command: {}", verify_cmd);
    let verify_out = interactor.cmd(&verify_cmd)?;

    // Clean up verify directory
    let _ = interactor.rm(&verify_dir);

    if verify_out.exit_code != 0 {
        let _ = interactor.rm(&local_path);

        anyhow::bail!(
            "pg_verifybackup verification failed with exit code {}: {}",
            verify_out.exit_code,
            verify_out.stderr.trim()
        );
    }

    // Adjust permissions so that the SSH user can read and download the backup files
    interactor.chmod(&local_path, "755")?;

    // 7. Get List of generated backup files to upload
    let files_list = interactor.ls(&local_path)?;
    for file in &files_list {
        let remote_filepath = format!("{}/{}", local_path, file);
        let temp_local_file = std::env::temp_dir().join(format!("crane-backup-{}-{}", id, file));

        // Download from VPS
        interactor.download(temp_local_file.to_str().unwrap(), &remote_filepath)?;

        // Read bytes and upload to S3
        let file_bytes = std::fs::read(&temp_local_file)?;
        let s3_key = format!("backups/{}/{}", id, file);
        s3_client.put_object(&s3_key, &file_bytes)?;

        // Clean up local temp file
        let _ = std::fs::remove_file(temp_local_file);
    }

    // 8. Create Backup Metadata
    let s3_path = format!("{}/backups/{}", bucket_name, id);
    let meta = BackupMetadata {
        id: id.clone(),
        date: date.clone(),
        time: time.clone(),
        taken_at: Some(format!("{} {}", date, time)),
        backup_type: if is_incr {
            "INCR".to_string()
        } else {
            "FULL".to_string()
        },
        base: base_id,
        local_path: local_path.clone(),
        s3_path,
        label: label.map(|s| s.to_string()),
    };

    // 9. Write metadata descriptor file locally and upload to S3
    let meta_toml = toml::to_string(&meta)?;
    interactor.create_file(&format!("{}/metadata.toml", local_path), &meta_toml)?;
    let metadata_file = format!("{}/metadata.toml", local_path);
    interactor.chown(&metadata_file, "postgres", "postgres")?;
    s3_client.put_object(
        &format!("backups/{}/metadata.toml", id),
        meta_toml.as_bytes(),
    )?;

    // 10. Update backup registry on S3 and local
    let registry_key = "backups/registry.toml";
    let backups = get_backups_data_from_s3(s3_client)?;
    let mut registry = BackupRegistry { backups };

    registry.backups.push(meta.clone());
    let registry_toml = toml::to_string(&registry)?;
    s3_client.put_object(registry_key, registry_toml.as_bytes())?;

    interactor.create_file("/var/lib/postgresql/backups/registry.toml", &registry_toml)?;
    interactor.chown(
        "/var/lib/postgresql/backups/registry.toml",
        "postgres",
        "postgres",
    )?;
    interactor.chmod("/var/lib/postgresql/backups/registry.toml", "644")?;

    // Force a WAL switch at the end of the backup to ensure that the WAL segment
    // active during the backup is archived and available for PITR.
    interactor.mkdir("/var/lib/postgresql/wal_archive")?;
    interactor.chown("/var/lib/postgresql/wal_archive", "postgres", "postgres")?;

    let switch_out = interactor.psql(
        Some("SELECT pg_walfile_name(pg_switch_wal() - 1);"),
        None,
        None,
        true,
    )?;
    let wal_filename = switch_out.stdout.trim().to_string();
    if wal_filename.is_empty() {
        anyhow::bail!("pg_switch_wal() returned empty filename");
    }
    println!(
        "Switched to new WAL. Switched segment filename: {}",
        wal_filename
    );

    // Diagnostic: check actual archive_mode at runtime
    if let Ok(am_out) = interactor.psql(Some("SHOW archive_mode;"), None, None, true) {
        println!("archive_mode = {}", am_out.stdout.trim());
    }
    if let Ok(ac_out) = interactor.psql(Some("SHOW archive_command;"), None, None, true) {
        println!("archive_command = {}", ac_out.stdout.trim());
    }

    let pgdata_dir = format!("/var/lib/postgresql/{}/main", pg_version);

    // Try immediate copy from pg_wal before the segment gets recycled
    let wal_source = format!("{}/pg_wal/{}", pgdata_dir, wal_filename);
    let wal_dest = format!("/var/lib/postgresql/wal_archive/{}", wal_filename);
    let immediate_cp = format!("sudo -u postgres cp {} {}", wal_source, wal_dest);
    match interactor.cmd(&immediate_cp) {
        Ok(out) if out.exit_code == 0 => {
            println!("WAL segment {} archived (immediate copy).", wal_filename);
        }

        _ => {
            // File may already be in wal_archive via the archiver, or already recycled
            if let Ok(exists) = interactor.exists(&wal_dest) {
                if exists {
                    println!("WAL segment {} already in archive.", wal_filename);
                } else {
                    println!(
                        "WARNING: WAL segment {} not found in pg_wal or wal_archive. PITR may fail.",
                        wal_filename
                    );
                }
            }
        }
    }

    println!(
        "\nBACKUP {} {} completed\n",
        id,
        meta.taken_at.as_deref().unwrap_or("unknown").to_string()
    );

    Ok(meta)
}
