use crate::{
    postgres_unit::entity::{BackupMetadata, BackupRegistry},
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
        anyhow::bail!("Failed to parse date output from server: '{}'", date_output.stdout);
    }
    let id = parts[0].to_string();
    let date = parts[1].to_string();
    let time = parts[2].to_string();

    let local_path = format!("/var/lib/postgresql/backups/{}", id);
    let pg_basebackup = format!("/usr/lib/postgresql/{}/bin/pg_basebackup", pg_version);
    let pg_verifybackup = format!("/usr/lib/postgresql/{}/bin/pg_verifybackup", pg_version);

    // 2. Ensure Backup Directories exist
    interactor.cmd("sudo mkdir -p /var/lib/postgresql/backups")?;
    interactor.cmd("sudo chown postgres:postgres /var/lib/postgresql/backups")?;
    interactor.cmd("sudo chmod 755 /var/lib/postgresql/backups")?;
    interactor.cmd(&format!("sudo -u postgres mkdir -p {}", local_path))?;

    // 3. Grant pg_read_server_files to replicator (idempotent)
    interactor.cmd(r#"sudo -u postgres psql -c "GRANT pg_read_server_files TO replicator;""#)?;

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
                interactor.cmd(&format!(
                    "sudo -u postgres mkdir -p /var/lib/postgresql/backups/{}",
                    parent.id
                ))?;
                interactor.cmd(&format!(
                    "sudo chmod 755 /var/lib/postgresql/backups/{}",
                    parent.id
                ))?;
                let s3_key = format!("backups/{}/backup_manifest", parent.id);
                let manifest_data = s3_client.get_object(&s3_key)?;

                // Write back on VPS
                let temp_path = format!("/tmp/manifest_{}", parent.id);
                let content = String::from_utf8_lossy(&manifest_data);
                interactor.create_file(&temp_path, &content)?;
                interactor.cmd(&format!("sudo mv {} {}", temp_path, parent_manifest))?;
                interactor.cmd(&format!("sudo chown postgres:postgres {}", parent_manifest))?;
                interactor.cmd(&format!("sudo chmod 644 {}", parent_manifest))?;
            }
            pgbasebackup_cmd.push_str(&format!(" --incremental={}", parent_manifest));
        } else {
            anyhow::bail!("Cannot perform incremental backup: no previous backup found.");
        }
    }

    // 5. Run pg_basebackup
    println!("Running pg_basebackup command: {}", pgbasebackup_cmd);
    if let Err(e) = interactor.cmd(&pgbasebackup_cmd) {
        let _ = interactor.cmd(&format!("sudo rm -rf {}", local_path));
        anyhow::bail!("pg_basebackup failed: {}", e);
    }

    // 6. Verify Backup (requires extracting tar to a temp directory first)
    let verify_dir = format!("/var/lib/postgresql/backups/{}_verify", id);
    interactor.cmd(&format!("sudo -u postgres mkdir -p {}", verify_dir))?;
    interactor.cmd(&format!(
        "sudo -u postgres tar -xf {}/base.tar -C {}",
        local_path, verify_dir
    ))?;

    // Check if pg_wal.tar exists and extract it
    let test_wal = interactor.cmd(&format!(
        "test -f {}/pg_wal.tar && echo 'yes' || echo 'no'",
        local_path
    ))?;
    if test_wal.stdout.trim() == "yes" {
        interactor.cmd(&format!("sudo -u postgres mkdir -p {}/pg_wal", verify_dir))?;
        interactor.cmd(&format!(
            "sudo -u postgres tar -xf {}/pg_wal.tar -C {}/pg_wal/",
            local_path, verify_dir
        ))?;
    }

    // Copy backup_manifest to verify_dir
    interactor.cmd(&format!(
        "sudo cp {}/backup_manifest {}/",
        local_path, verify_dir
    ))?;
    interactor.cmd(&format!("sudo chown -R postgres:postgres {}", verify_dir))?;

    let verify_cmd = format!("sudo -u postgres {} {}", pg_verifybackup, verify_dir);
    println!("Running verifybackup command: {}", verify_cmd);
    let verify_result = interactor.cmd(&verify_cmd);

    // Clean up verify directory
    let _ = interactor.cmd(&format!("sudo rm -rf {}", verify_dir));

    if let Err(e) = verify_result {
        let _ = interactor.cmd(&format!("sudo rm -rf {}", local_path));
        anyhow::bail!("pg_verifybackup verification failed: {}", e);
    }

    // Adjust permissions so that the SSH user can read and download the backup files
    interactor.cmd(&format!("sudo chmod -R 755 {}", local_path))?;

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
        date,
        time,
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
    let temp_meta_path = format!("/tmp/metadata_{}.toml", id);
    interactor.create_file(&temp_meta_path, &meta_toml)?;
    interactor.cmd(&format!(
        "sudo mv {} {}/metadata.toml",
        temp_meta_path, local_path
    ))?;
    interactor.cmd(&format!(
        "sudo chown postgres:postgres {}/metadata.toml",
        local_path
    ))?;
    s3_client.put_object(
        &format!("backups/{}/metadata.toml", id),
        meta_toml.as_bytes(),
    )?;

    // 10. Update backup registry on S3 and local
    let registry_key = "backups/registry.toml";
    let mut registry = match s3_client.get_object(registry_key) {
        Ok(data) => {
            let content = String::from_utf8_lossy(&data).to_string();
            toml::from_str::<BackupRegistry>(&content).unwrap_or_default()
        }
        Err(_) => BackupRegistry::default(),
    };

    registry.backups.push(meta.clone());
    let registry_toml = toml::to_string(&registry)?;
    s3_client.put_object(registry_key, registry_toml.as_bytes())?;

    let temp_reg_path = format!("/tmp/registry_{}.toml", id);
    interactor.create_file(&temp_reg_path, &registry_toml)?;
    interactor.cmd(&format!(
        "sudo mv {} /var/lib/postgresql/backups/registry.toml",
        temp_reg_path
    ))?;
    interactor.cmd("sudo chown postgres:postgres /var/lib/postgresql/backups/registry.toml")?;
    interactor.cmd("sudo chmod 644 /var/lib/postgresql/backups/registry.toml")?;

    Ok(meta)
}

pub fn run_restore(
    interactor: &dyn ServerInteractor,
    s3_client: &dyn S3Client,
    pg_version: &str,
    backup: &BackupMetadata,
    chain: &[BackupMetadata],
) -> anyhow::Result<()> {
    let pg_ctl = format!("/usr/lib/postgresql/{}/bin/pg_ctl", pg_version);
    let pg_combinebackup = format!("/usr/lib/postgresql/{}/bin/pg_combinebackup", pg_version);
    let pg_verifybackup = format!("/usr/lib/postgresql/{}/bin/pg_verifybackup", pg_version);
    let pgdata_dir = format!("/var/lib/postgresql/{}/main", pg_version);

    // 1. Stop PostgreSQL service
    let _ = interactor.cmd("sudo systemctl stop postgresql --no-block");
    let _ = interactor.cmd(&format!(
        "sudo systemctl stop postgresql@{}-main --no-block",
        pg_version
    ));
    let _ = interactor.cmd(&format!(
        "sudo -u postgres {} -D {} stop -m immediate",
        pg_ctl, pgdata_dir
    ));

    // 2. Download all backups in the chain from S3 to VPS local backups dir
    interactor.cmd("sudo mkdir -p /var/lib/postgresql/backups")?;
    interactor.cmd("sudo chown postgres:postgres /var/lib/postgresql/backups")?;
    interactor.cmd("sudo chmod 755 /var/lib/postgresql/backups")?;

    for item in chain {
        let remote_dir = format!("/var/lib/postgresql/backups/{}", item.id);
        interactor.cmd(&format!("sudo -u postgres mkdir -p {}", remote_dir))?;
        interactor.cmd(&format!("sudo chmod 755 {}", remote_dir))?;

        let files = vec!["base.tar", "backup_manifest", "pg_wal.tar"];
        for file in files {
            let s3_key = format!("backups/{}/{}", item.id, file);
            if let Ok(data) = s3_client.get_object(&s3_key) {
                let temp_path =
                    std::env::temp_dir().join(format!("crane-restore-{}-{}", item.id, file));
                std::fs::write(&temp_path, &data)?;

                let remote_temp_file = format!("/tmp/crane-restore-{}-{}", item.id, file);
                interactor.upload(temp_path.to_str().unwrap(), &remote_temp_file)?;
                let _ = std::fs::remove_file(temp_path);

                let remote_file = format!("{}/{}", remote_dir, file);
                interactor.cmd(&format!("sudo mv {} {}", remote_temp_file, remote_file))?;
                interactor.cmd(&format!("sudo chown postgres:postgres {}", remote_file))?;
                interactor.cmd(&format!("sudo chmod 644 {}", remote_file))?;
            }
        }
    }

    if chain.len() <= 1 {
        // 3. Clear data directory
        interactor.cmd(&format!("sudo rm -rf {}", pgdata_dir))?;
        interactor.cmd(&format!("sudo -u postgres mkdir -p {}", pgdata_dir))?;
        interactor.cmd(&format!("sudo chmod 700 {}", pgdata_dir))?;

        // 4. Extract base.tar
        interactor.cmd(&format!(
            "sudo -u postgres tar -xf /var/lib/postgresql/backups/{}/base.tar -C {}",
            backup.id, pgdata_dir
        ))?;

        // 5. Extract pg_wal.tar if present
        let test_wal = interactor.cmd(&format!(
            "test -f /var/lib/postgresql/backups/{}/pg_wal.tar && echo 'yes' || echo 'no'",
            backup.id
        ))?;
        if test_wal.stdout.trim() == "yes" {
            interactor.cmd(&format!("sudo -u postgres mkdir -p {}/pg_wal", pgdata_dir))?;
            interactor.cmd(&format!(
                "sudo -u postgres tar -xf /var/lib/postgresql/backups/{}/pg_wal.tar -C {}/pg_wal/",
                backup.id, pgdata_dir
            ))?;
        }
    } else {
        // 3. Extract all backups in the chain to separate folders
        for item in chain {
            let extracted_dir = format!("/var/lib/postgresql/backups/{}_extracted", item.id);
            interactor.cmd(&format!("sudo rm -rf {}", extracted_dir))?;
            interactor.cmd(&format!("sudo -u postgres mkdir -p {}", extracted_dir))?;
            interactor.cmd(&format!(
                "sudo -u postgres tar -xf /var/lib/postgresql/backups/{}/base.tar -C {}",
                item.id, extracted_dir
            ))?;

            // Copy backup_manifest to extracted directory so pg_combinebackup can find it
            interactor.cmd(&format!(
                "sudo cp /var/lib/postgresql/backups/{}/backup_manifest {}/",
                item.id, extracted_dir
            ))?;
            interactor.cmd(&format!(
                "sudo chown postgres:postgres {}/backup_manifest",
                extracted_dir
            ))?;
            interactor.cmd(&format!("sudo chmod 644 {}/backup_manifest", extracted_dir))?;
        }

        // 4. Combine backups
        let combined_dir = "/var/lib/postgresql/backups/combined";
        interactor.cmd(&format!("sudo rm -rf {}", combined_dir))?;

        let mut combine_cmd = format!("sudo -u postgres {} ", pg_combinebackup);
        for item in chain {
            combine_cmd.push_str(&format!(
                "/var/lib/postgresql/backups/{}_extracted ",
                item.id
            ));
        }
        combine_cmd.push_str(&format!("-o {}", combined_dir));
        interactor.cmd(&combine_cmd)?;

        // Extract target backup's pg_wal.tar to combined_dir/pg_wal if present
        let test_wal = interactor.cmd(&format!(
            "test -f /var/lib/postgresql/backups/{}/pg_wal.tar && echo 'yes' || echo 'no'",
            backup.id
        ))?;
        if test_wal.stdout.trim() == "yes" {
            interactor.cmd(&format!(
                "sudo -u postgres mkdir -p {}/pg_wal",
                combined_dir
            ))?;
            interactor.cmd(&format!(
                "sudo -u postgres tar -xf /var/lib/postgresql/backups/{}/pg_wal.tar -C {}/pg_wal/",
                backup.id, combined_dir
            ))?;
        }

        // 5. Verify the combined backup
        let verify_cmd = format!("sudo -u postgres {} {}", pg_verifybackup, combined_dir);
        interactor.cmd(&verify_cmd)?;

        // 6. Clear and replace data directory with combined backup
        interactor.cmd(&format!("sudo rm -rf {}", pgdata_dir))?;
        interactor.cmd(&format!("sudo mv {} {}", combined_dir, pgdata_dir))?;

        // Clean up extracted directories
        for item in chain {
            let extracted_dir = format!("/var/lib/postgresql/backups/{}_extracted", item.id);
            let _ = interactor.cmd(&format!("sudo rm -rf {}", extracted_dir));
        }
    }

    // 6. Set ownership and start service
    interactor.cmd(&format!("sudo chown -R postgres:postgres {}", pgdata_dir))?;
    interactor.cmd(&format!("sudo chmod 700 {}", pgdata_dir))?;

    // Restart postgres service using direct pg_ctl command (matching setup.rs follower setup)
    let start_cmd = format!(
        "sudo -u postgres {} -D {} -o \"-c config_file=/etc/postgresql/{}/main/postgresql.conf\" start > /dev/null 2>&1 < /dev/null",
        pg_ctl, pgdata_dir, pg_version
    );
    interactor.cmd(&start_cmd)?;

    Ok(())
}
