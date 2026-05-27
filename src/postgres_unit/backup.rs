use crate::{
    postgres_unit::{
        entity::{BackupMetadata, BackupRegistry},
        helper::{cmdw, get_backups_from_s3},
    },
    s3::s3_client::S3Client,
    server_interactor::server_interactor_trait::ServerInteractor,
};

pub fn run_backup(
    interactor: &dyn ServerInteractor,
    s3_client: &dyn S3Client,
    pg_version: &str,
    backup_type: &str,
    replica_pass: &str,
    bucket_name: &str,
    last_backup: Option<&BackupMetadata>,
) -> anyhow::Result<BackupMetadata> {
    // 1. Get Date and Time from DB Node
    let date_output = interactor.cmd("date +'%Y%m%d%H%M%S%3N %Y-%m-%d %H:%M:%S'")?;
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
    cmdw(interactor, "sudo mkdir -p /var/lib/postgresql/backups")?;
    cmdw(
        interactor,
        "sudo chown postgres:postgres /var/lib/postgresql/backups",
    )?;
    cmdw(interactor, "sudo chmod 755 /var/lib/postgresql/backups")?;
    cmdw(
        interactor,
        &format!("sudo -u postgres mkdir -p {}", local_path),
    )?;

    // 3. Grant pg_read_server_files to replicator (idempotent)
    cmdw(
        interactor,
        r#"sudo -u postgres psql -c "GRANT pg_read_server_files TO replicator;""#,
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
            let test_manifest = interactor.cmd(&format!(
                "test -f {} && echo 'yes' || echo 'no'",
                parent_manifest
            ))?;
            if test_manifest.stdout.trim() != "yes" {
                // Recreate parent directory and restore manifest from S3
                cmdw(
                    interactor,
                    &format!(
                        "sudo -u postgres mkdir -p /var/lib/postgresql/backups/{}",
                        parent.id
                    ),
                )?;
                cmdw(
                    interactor,
                    &format!("sudo chmod 755 /var/lib/postgresql/backups/{}", parent.id),
                )?;
                let s3_key = format!("backups/{}/backup_manifest", parent.id);
                let manifest_data = s3_client.get_object(&s3_key)?;

                // Write back on VPS
                let content = String::from_utf8_lossy(&manifest_data);
                interactor.create_file(&parent_manifest, &content)?;
                cmdw(
                    interactor,
                    &format!("sudo chown postgres:postgres {}", parent_manifest),
                )?;
                cmdw(interactor, &format!("sudo chmod 644 {}", parent_manifest))?;
            }
            pgbasebackup_cmd.push_str(&format!(" --incremental={}", parent_manifest));
        } else {
            anyhow::bail!("Cannot perform incremental backup: no previous backup found.");
        }
    }

    // 5. Run pg_basebackup
    println!("\nRunning pg_basebackup command: {}", pgbasebackup_cmd);
    let pgbasebackup_out = interactor.cmd(&pgbasebackup_cmd)?;
    if pgbasebackup_out.exit_code != 0 {
        let _ = interactor.cmd(&format!("sudo rm -rf {}", local_path));
        anyhow::bail!(
            "pg_basebackup failed with exit code {}: {}",
            pgbasebackup_out.exit_code,
            pgbasebackup_out.stderr.trim()
        );
    }

    // 6. Verify Backup (requires extracting tar to a temp directory first)
    let verify_dir = format!("/var/lib/postgresql/backups/{}_verify", id);
    cmdw(
        interactor,
        &format!("sudo -u postgres mkdir -p {}", verify_dir),
    )?;
    cmdw(
        interactor,
        &format!(
            "sudo -u postgres tar -xf {}/base.tar -C {}",
            local_path, verify_dir
        ),
    )?;

    // Check if pg_wal.tar exists and extract it
    let test_wal = interactor.cmd(&format!(
        "test -f {}/pg_wal.tar && echo 'yes' || echo 'no'",
        local_path
    ))?;
    if test_wal.stdout.trim() == "yes" {
        cmdw(
            interactor,
            &format!("sudo -u postgres mkdir -p {}/pg_wal", verify_dir),
        )?;
        cmdw(
            interactor,
            &format!(
                "sudo -u postgres tar -xf {}/pg_wal.tar -C {}/pg_wal/",
                local_path, verify_dir
            ),
        )?;
    }

    // Copy backup_manifest to verify_dir
    cmdw(
        interactor,
        &format!("sudo cp {}/backup_manifest {}/", local_path, verify_dir),
    )?;
    cmdw(
        interactor,
        &format!("sudo chown -R postgres:postgres {}", verify_dir),
    )?;

    let verify_cmd = format!("sudo -u postgres {} {}", pg_verifybackup, verify_dir);
    println!("\nRunning pg_verifybackup command: {}", verify_cmd);
    let verify_out = interactor.cmd(&verify_cmd)?;

    // Clean up verify directory
    let _ = interactor.cmd(&format!("sudo rm -rf {}", verify_dir));

    if verify_out.exit_code != 0 {
        let _ = interactor.cmd(&format!("sudo rm -rf {}", local_path));
        anyhow::bail!(
            "pg_verifybackup verification failed with exit code {}: {}",
            verify_out.exit_code,
            verify_out.stderr.trim()
        );
    }

    // Adjust permissions so that the SSH user can read and download the backup files
    cmdw(interactor, &format!("sudo chmod -R 755 {}", local_path))?;

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
    };

    // 9. Write metadata descriptor file locally and upload to S3
    let meta_toml = toml::to_string(&meta)?;
    interactor.create_file(&format!("{}/metadata.toml", local_path), &meta_toml)?;
    cmdw(
        interactor,
        &format!("sudo chown postgres:postgres {}/metadata.toml", local_path),
    )?;
    s3_client.put_object(
        &format!("backups/{}/metadata.toml", id),
        meta_toml.as_bytes(),
    )?;

    // 10. Update backup registry on S3 and local
    let registry_key = "backups/registry.toml";
    let backups = get_backups_from_s3(s3_client)?;
    let mut registry = BackupRegistry { backups };

    registry.backups.push(meta.clone());
    let registry_toml = toml::to_string(&registry)?;
    s3_client.put_object(registry_key, registry_toml.as_bytes())?;

    interactor.create_file("/var/lib/postgresql/backups/registry.toml", &registry_toml)?;
    cmdw(
        interactor,
        "sudo chown postgres:postgres /var/lib/postgresql/backups/registry.toml",
    )?;
    cmdw(
        interactor,
        "sudo chmod 644 /var/lib/postgresql/backups/registry.toml",
    )?;

    println!("\nBACKUP {} {:?} completed\n", id, meta.taken_at);

    Ok(meta)
}
