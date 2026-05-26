use crate::{
    postgres_unit::entity::{BackupMetadata, BackupRegistry},
    s3::s3_client::S3Client,
    server_interactor::server_interactor_trait::ServerInteractor,
};

fn run_cmd(
    interactor: &dyn ServerInteractor,
    command: &str,
) -> anyhow::Result<crate::ssh::CmdOutput> {
    let out = interactor.cmd(command)?;
    if out.exit_code != 0 {
        anyhow::bail!(
            "Command '{}' failed with exit code {}: {}",
            command,
            out.exit_code,
            out.stderr.trim()
        );
    }
    Ok(out)
}

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
    run_cmd(interactor, "sudo mkdir -p /var/lib/postgresql/backups")?;
    run_cmd(
        interactor,
        "sudo chown postgres:postgres /var/lib/postgresql/backups",
    )?;
    run_cmd(interactor, "sudo chmod 755 /var/lib/postgresql/backups")?;
    run_cmd(
        interactor,
        &format!("sudo -u postgres mkdir -p {}", local_path),
    )?;

    // 3. Grant pg_read_server_files to replicator (idempotent)
    run_cmd(
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
                run_cmd(
                    interactor,
                    &format!(
                        "sudo -u postgres mkdir -p /var/lib/postgresql/backups/{}",
                        parent.id
                    ),
                )?;
                run_cmd(
                    interactor,
                    &format!("sudo chmod 755 /var/lib/postgresql/backups/{}", parent.id),
                )?;
                let s3_key = format!("backups/{}/backup_manifest", parent.id);
                let manifest_data = s3_client.get_object(&s3_key)?;

                // Write back on VPS
                let temp_path = format!("/tmp/manifest_{}", parent.id);
                let content = String::from_utf8_lossy(&manifest_data);
                interactor.create_file(&temp_path, &content)?;
                run_cmd(
                    interactor,
                    &format!("sudo mv {} {}", temp_path, parent_manifest),
                )?;
                run_cmd(
                    interactor,
                    &format!("sudo chown postgres:postgres {}", parent_manifest),
                )?;
                run_cmd(interactor, &format!("sudo chmod 644 {}", parent_manifest))?;
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
    run_cmd(
        interactor,
        &format!("sudo -u postgres mkdir -p {}", verify_dir),
    )?;
    run_cmd(
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
        run_cmd(
            interactor,
            &format!("sudo -u postgres mkdir -p {}/pg_wal", verify_dir),
        )?;
        run_cmd(
            interactor,
            &format!(
                "sudo -u postgres tar -xf {}/pg_wal.tar -C {}/pg_wal/",
                local_path, verify_dir
            ),
        )?;
    }

    // Copy backup_manifest to verify_dir
    run_cmd(
        interactor,
        &format!("sudo cp {}/backup_manifest {}/", local_path, verify_dir),
    )?;
    run_cmd(
        interactor,
        &format!("sudo chown -R postgres:postgres {}", verify_dir),
    )?;

    let verify_cmd = format!("sudo -u postgres {} {}", pg_verifybackup, verify_dir);
    println!("\nRunning verifybackup command: {}", verify_cmd);
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
    run_cmd(interactor, &format!("sudo chmod -R 755 {}", local_path))?;

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
    let temp_meta_path = format!("/tmp/metadata_{}.toml", id);
    interactor.create_file(&temp_meta_path, &meta_toml)?;
    run_cmd(
        interactor,
        &format!("sudo mv {} {}/metadata.toml", temp_meta_path, local_path),
    )?;
    run_cmd(
        interactor,
        &format!("sudo chown postgres:postgres {}/metadata.toml", local_path),
    )?;
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
    run_cmd(
        interactor,
        &format!(
            "sudo mv {} /var/lib/postgresql/backups/registry.toml",
            temp_reg_path
        ),
    )?;
    run_cmd(
        interactor,
        "sudo chown postgres:postgres /var/lib/postgresql/backups/registry.toml",
    )?;
    run_cmd(
        interactor,
        "sudo chmod 644 /var/lib/postgresql/backups/registry.toml",
    )?;

    Ok(meta)
}

pub fn run_restore(
    interactor: &dyn ServerInteractor,
    s3_client: &dyn S3Client,
    pg_version: &str,
    backup: &BackupMetadata,
    chain: &[BackupMetadata],
    pitr_time: Option<&str>, // "YYYY-MM-DD HH:MM:SS" UTC — None = regular restore
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
    run_cmd(interactor, "sudo mkdir -p /var/lib/postgresql/backups")?;
    run_cmd(
        interactor,
        "sudo chown postgres:postgres /var/lib/postgresql/backups",
    )?;
    run_cmd(interactor, "sudo chmod 755 /var/lib/postgresql/backups")?;

    for item in chain {
        let remote_dir = format!("/var/lib/postgresql/backups/{}", item.id);
        run_cmd(
            interactor,
            &format!("sudo -u postgres mkdir -p {}", remote_dir),
        )?;
        run_cmd(interactor, &format!("sudo chmod 755 {}", remote_dir))?;

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
            run_cmd(
                interactor,
                &format!("sudo mv {} {}", remote_temp_file, remote_file),
            )?;
            run_cmd(
                interactor,
                &format!("sudo chown postgres:postgres {}", remote_file),
            )?;
            run_cmd(interactor, &format!("sudo chmod 644 {}", remote_file))?;
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
            run_cmd(
                interactor,
                &format!("sudo mv {} {}", remote_temp_file, remote_file),
            )?;
            run_cmd(
                interactor,
                &format!("sudo chown postgres:postgres {}", remote_file),
            )?;
            run_cmd(interactor, &format!("sudo chmod 644 {}", remote_file))?;
        }
    }

    if chain.len() <= 1 {
        // 3. Clear data directory
        run_cmd(interactor, &format!("sudo rm -rf {}", pgdata_dir))?;
        run_cmd(
            interactor,
            &format!("sudo -u postgres mkdir -p {}", pgdata_dir),
        )?;
        run_cmd(interactor, &format!("sudo chmod 700 {}", pgdata_dir))?;

        // 4. Extract base.tar
        run_cmd(
            interactor,
            &format!(
                "sudo -u postgres tar -xf /var/lib/postgresql/backups/{}/base.tar -C {}",
                backup.id, pgdata_dir
            ),
        )?;

        // 5. Extract pg_wal.tar if present
        let test_wal = interactor.cmd(&format!(
            "test -f /var/lib/postgresql/backups/{}/pg_wal.tar && echo 'yes' || echo 'no'",
            backup.id
        ))?;
        if test_wal.stdout.trim() == "yes" {
            run_cmd(
                interactor,
                &format!("sudo -u postgres mkdir -p {}/pg_wal", pgdata_dir),
            )?;
            run_cmd(
                interactor,
                &format!(
                    "sudo -u postgres tar -xf /var/lib/postgresql/backups/{}/pg_wal.tar -C {}/pg_wal/",
                    backup.id, pgdata_dir
                ),
            )?;
        }
    } else {
        // 3. Extract all backups in the chain to separate folders
        for item in chain {
            let extracted_dir = format!("/var/lib/postgresql/backups/{}_extracted", item.id);
            run_cmd(interactor, &format!("sudo rm -rf {}", extracted_dir))?;
            run_cmd(
                interactor,
                &format!("sudo -u postgres mkdir -p {}", extracted_dir),
            )?;
            run_cmd(
                interactor,
                &format!(
                    "sudo -u postgres tar -xf /var/lib/postgresql/backups/{}/base.tar -C {}",
                    item.id, extracted_dir
                ),
            )?;

            // Copy backup_manifest to extracted directory so pg_combinebackup can find it
            run_cmd(
                interactor,
                &format!(
                    "sudo cp /var/lib/postgresql/backups/{}/backup_manifest {}/",
                    item.id, extracted_dir
                ),
            )?;
            run_cmd(
                interactor,
                &format!(
                    "sudo chown postgres:postgres {}/backup_manifest",
                    extracted_dir
                ),
            )?;
            run_cmd(
                interactor,
                &format!("sudo chmod 644 {}/backup_manifest", extracted_dir),
            )?;
        }

        // 4. Combine backups
        let combined_dir = "/var/lib/postgresql/backups/combined";
        run_cmd(interactor, &format!("sudo rm -rf {}", combined_dir))?;

        let mut combine_cmd = format!("sudo -u postgres {} ", pg_combinebackup);
        for item in chain {
            combine_cmd.push_str(&format!(
                "/var/lib/postgresql/backups/{}_extracted ",
                item.id
            ));
        }
        combine_cmd.push_str(&format!("-o {}", combined_dir));
        run_cmd(interactor, &combine_cmd)?;

        // Extract target backup's pg_wal.tar to combined_dir/pg_wal if present
        let test_wal = interactor.cmd(&format!(
            "test -f /var/lib/postgresql/backups/{}/pg_wal.tar && echo 'yes' || echo 'no'",
            backup.id
        ))?;
        if test_wal.stdout.trim() == "yes" {
            run_cmd(
                interactor,
                &format!("sudo -u postgres mkdir -p {}/pg_wal", combined_dir),
            )?;
            run_cmd(
                interactor,
                &format!(
                    "sudo -u postgres tar -xf /var/lib/postgresql/backups/{}/pg_wal.tar -C {}/pg_wal/",
                    backup.id, combined_dir
                ),
            )?;
        }

        // 5. Verify the combined backup
        let verify_cmd = format!("sudo -u postgres {} {}", pg_verifybackup, combined_dir);
        run_cmd(interactor, &verify_cmd)?;

        // 6. Clear and replace data directory with combined backup
        run_cmd(interactor, &format!("sudo rm -rf {}", pgdata_dir))?;
        run_cmd(
            interactor,
            &format!("sudo mv {} {}", combined_dir, pgdata_dir),
        )?;

        // Clean up extracted directories
        for item in chain {
            let extracted_dir = format!("/var/lib/postgresql/backups/{}_extracted", item.id);
            let _ = interactor.cmd(&format!("sudo rm -rf {}", extracted_dir));
        }
    }

    // 6. Set ownership and start service
    run_cmd(
        interactor,
        &format!("sudo chown -R postgres:postgres {}", pgdata_dir),
    )?;
    run_cmd(interactor, &format!("sudo chmod 700 {}", pgdata_dir))?;

    if let Some(target_time) = pitr_time {
        // Create recovery.signal (PG12+ triggers archive recovery mode)
        run_cmd(
            interactor,
            &format!("sudo -u postgres touch {}/recovery.signal", pgdata_dir),
        )?;
        // Start without restore_command=false override so PITR WAL replay can proceed
        let start_cmd = format!(
            "sudo -u postgres {} -D {} -o \"-c config_file=/etc/postgresql/{}/main/postgresql.conf -c restore_command='cp /var/lib/postgresql/wal_archive/%f %p' -c recovery_target_time='{}' -c recovery_target_action=promote -c recovery_target_inclusive=on\" start > /dev/null 2>&1 < /dev/null",
            pg_ctl, pgdata_dir, pg_version, target_time
        );
        let out = interactor.cmd(&start_cmd)?;
        if out.exit_code != 0 {
            anyhow::bail!(
                "Failed to start PostgreSQL with PITR (exit code {}): {}",
                out.exit_code,
                out.stderr
            );
        }
    } else {
        // Regular restore: suppress archive recovery with restore_command=false
        let start_cmd = format!(
            "sudo -u postgres {} -D {} -o \"-c config_file=/etc/postgresql/{}/main/postgresql.conf -c restore_command=false\" start > /dev/null 2>&1 < /dev/null",
            pg_ctl, pgdata_dir, pg_version
        );
        let out = interactor.cmd(&start_cmd)?;
        if out.exit_code != 0 {
            anyhow::bail!(
                "Failed to start PostgreSQL (exit code {}): {}",
                out.exit_code,
                out.stderr
            );
        }
    }

    Ok(())
}
