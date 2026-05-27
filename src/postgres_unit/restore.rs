use crate::{
    helper::base64::base64_encode,
    postgres_unit::{
        entity::BackupMetadata,
        helper::{cmdw, debug_get_postgres_logs, is_postgres_running},
    },
    s3::s3_client::S3Client,
    server_interactor::server_interactor_trait::ServerInteractor,
};

pub fn run_restore(
    interactor: &dyn ServerInteractor,
    s3_client: &dyn S3Client,
    pg_version: &str,
    backup: &BackupMetadata,
    chain: &[BackupMetadata],
    pitr_time: Option<&str>, // "YYYY-MM-DD HH:MM:SS" UTC — None = regular restore
) -> anyhow::Result<()> {
    let mut chain = chain.to_vec();
    let mut backup = backup.clone();

    if let Some(pitr) = pitr_time {
        let pitr_dt = chrono::NaiveDateTime::parse_from_str(pitr, "%Y-%m-%d %H:%M:%S")
            .map_err(|_| anyhow::anyhow!("--pitr must be in 'YYYY-MM-DD HH:MM:SS' format"))?;

        let mut filtered_chain = Vec::new();
        for item in &chain {
            if let Some(ref taken_at) = item.taken_at {
                let backup_dt =
                    chrono::NaiveDateTime::parse_from_str(taken_at, "%Y-%m-%d %H:%M:%S").map_err(
                        |_| anyhow::anyhow!("Backup has invalid taken_at: '{}'", taken_at),
                    )?;
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
    cmdw(interactor, "sudo mkdir -p /var/lib/postgresql/backups")?;
    cmdw(
        interactor,
        "sudo chown postgres:postgres /var/lib/postgresql/backups",
    )?;
    cmdw(interactor, "sudo chmod 755 /var/lib/postgresql/backups")?;

    for item in &chain {
        let remote_dir = format!("/var/lib/postgresql/backups/{}", item.id);
        cmdw(
            interactor,
            &format!("sudo -u postgres mkdir -p {}", remote_dir),
        )?;
        cmdw(interactor, &format!("sudo chmod 755 {}", remote_dir))?;

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
            cmdw(
                interactor,
                &format!("sudo mv {} {}", remote_temp_file, remote_file),
            )?;
            cmdw(
                interactor,
                &format!("sudo chown postgres:postgres {}", remote_file),
            )?;
            cmdw(interactor, &format!("sudo chmod 644 {}", remote_file))?;
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
            cmdw(
                interactor,
                &format!("sudo mv {} {}", remote_temp_file, remote_file),
            )?;
            cmdw(
                interactor,
                &format!("sudo chown postgres:postgres {}", remote_file),
            )?;
            cmdw(interactor, &format!("sudo chmod 644 {}", remote_file))?;
        }
    }

    if chain.len() <= 1 {
        // 3. Clear data directory
        cmdw(interactor, &format!("sudo rm -rf {}", pgdata_dir))?;
        cmdw(
            interactor,
            &format!("sudo -u postgres mkdir -p {}", pgdata_dir),
        )?;
        cmdw(interactor, &format!("sudo chmod 700 {}", pgdata_dir))?;

        // 4. Extract base.tar
        cmdw(
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
            cmdw(
                interactor,
                &format!("sudo -u postgres mkdir -p {}/pg_wal", pgdata_dir),
            )?;

            cmdw(
                interactor,
                &format!(
                    "sudo -u postgres tar -xf /var/lib/postgresql/backups/{}/pg_wal.tar -C {}/pg_wal/",
                    backup.id, pgdata_dir
                ),
            )?;
        }
    } else {
        // 3. Extract all backups in the chain to separate folders
        for item in &chain {
            let extracted_dir = format!("/var/lib/postgresql/backups/{}_extracted", item.id);
            cmdw(interactor, &format!("sudo rm -rf {}", extracted_dir))?;
            cmdw(
                interactor,
                &format!("sudo -u postgres mkdir -p {}", extracted_dir),
            )?;
            cmdw(
                interactor,
                &format!(
                    "sudo -u postgres tar -xf /var/lib/postgresql/backups/{}/base.tar -C {}",
                    item.id, extracted_dir
                ),
            )?;

            // Copy backup_manifest to extracted directory so pg_combinebackup can find it
            cmdw(
                interactor,
                &format!(
                    "sudo cp /var/lib/postgresql/backups/{}/backup_manifest {}/",
                    item.id, extracted_dir
                ),
            )?;
            cmdw(
                interactor,
                &format!(
                    "sudo chown postgres:postgres {}/backup_manifest",
                    extracted_dir
                ),
            )?;
            cmdw(
                interactor,
                &format!("sudo chmod 644 {}/backup_manifest", extracted_dir),
            )?;
        }

        // 4. Combine backups
        let combined_dir = "/var/lib/postgresql/backups/combined";
        cmdw(interactor, &format!("sudo rm -rf {}", combined_dir))?;

        let mut combine_cmd = format!("sudo -u postgres {} ", pg_combinebackup);
        for item in &chain {
            combine_cmd.push_str(&format!(
                "/var/lib/postgresql/backups/{}_extracted ",
                item.id
            ));
        }
        combine_cmd.push_str(&format!("-o {}", combined_dir));
        cmdw(interactor, &combine_cmd)?;

        // Extract target backup's pg_wal.tar to combined_dir/pg_wal if present
        let test_wal = interactor.cmd(&format!(
            "test -f /var/lib/postgresql/backups/{}/pg_wal.tar && echo 'yes' || echo 'no'",
            backup.id
        ))?;
        if test_wal.stdout.trim() == "yes" {
            cmdw(
                interactor,
                &format!("sudo -u postgres mkdir -p {}/pg_wal", combined_dir),
            )?;
            cmdw(
                interactor,
                &format!(
                    "sudo -u postgres tar -xf /var/lib/postgresql/backups/{}/pg_wal.tar -C {}/pg_wal/",
                    backup.id, combined_dir
                ),
            )?;
        }

        // 5. Verify the combined backup
        let verify_cmd = format!("sudo -u postgres {} {}", pg_verifybackup, combined_dir);
        cmdw(interactor, &verify_cmd)?;

        // 6. Clear and replace data directory with combined backup
        cmdw(interactor, &format!("sudo rm -rf {}", pgdata_dir))?;
        cmdw(
            interactor,
            &format!("sudo mv {} {}", combined_dir, pgdata_dir),
        )?;

        // Clean up extracted directories
        for item in &chain {
            let extracted_dir = format!("/var/lib/postgresql/backups/{}_extracted", item.id);
            let _ = interactor.cmd(&format!("sudo rm -rf {}", extracted_dir));
        }
    }

    // 6. Set ownership and start service
    cmdw(
        interactor,
        &format!("sudo chown -R postgres:postgres {}", pgdata_dir),
    )?;
    cmdw(interactor, &format!("sudo chmod 700 {}", pgdata_dir))?;

    if let Some(target_time) = pitr_time {
        // Write PITR settings to postgresql.auto.conf in pgdata_dir to avoid quoting issues
        let pitr_conf_path = format!("{}/postgresql.auto.conf", pgdata_dir);
        let pitr_conf_content = format!(
            "restore_command = 'cp /var/lib/postgresql/wal_archive/%f %p'\nrecovery_target_time = '{}'\nrecovery_target_action = promote\nrecovery_target_inclusive = on\nrecovery_target_timeline = 'current'\n",
            target_time
        );
        // Write via base64 to avoid any shell quoting issues with single quotes
        println!("writing PITR config at: {}", pitr_conf_path);
        let b64 = base64_encode(&pitr_conf_content);
        cmdw(
            interactor,
            &format!(
                "echo {} | base64 -d | sudo -u postgres tee -a {} > /dev/null",
                b64, pitr_conf_path
            ),
        )?;

        // Create recovery.signal (PG12+ triggers archive recovery mode)
        println!("writing recovery signal at: {}", pgdata_dir);
        cmdw(
            interactor,
            &format!("sudo -u postgres touch {}/recovery.signal", pgdata_dir),
        )?;

        // Start postgres; log to file so we can read errors
        let log_file = "/tmp/crane-pitr-pg-start.log";
        let start_cmd = format!(
            "sudo -u postgres {} -D {} -l {} -o \"-c config_file=/etc/postgresql/{}/main/postgresql.conf\" start",
            pg_ctl, pgdata_dir, log_file, pg_version
        );

        let out = interactor.cmd(&start_cmd)?;
        if out.exit_code != 0 {
            // Capture pg log for diagnosis
            // let mut log = interactor
            //     .cmd(&format!("sudo cat {} 2>/dev/null || true", log_file))
            //     .map(|o| o.stdout)
            //     .unwrap_or_default();

            // let extra_logs = get_last_postgres_logs(interactor, pg_version);
            // if !extra_logs.is_empty() {
            //     log.push_str(&extra_logs);
            // }

            // Clean up: try to restore/clean postgresql.auto.conf
            let _ = interactor.cmd(&format!(
                "sudo -u postgres sed -i '/restore_command/d;/recovery_target/d' {}",
                pitr_conf_path
            ));

            println!("\nstart command: {}\n", start_cmd);
            println!("stdout: {}\n", out.stdout);
            println!("stderr: {}\n\n", out.stderr);

            anyhow::bail!(
                "Failed to start PostgreSQL with PITR (exit code {})",
                out.exit_code
            );
        }

        // Wait for recovery to complete (postgres promotes itself)
        let mut ready = false;
        for _ in 0..30 {
            std::thread::sleep(std::time::Duration::from_millis(500));

            if is_postgres_running(interactor, pg_version) {
                ready = true;
                break;
            }
        }
        // Clean up PITR settings from postgresql.auto.conf
        let _ = interactor.cmd(&format!(
            "sudo -u postgres sed -i '/restore_command/d;/recovery_target/d' {}",
            pitr_conf_path
        ));

        if !ready {
            let mut log = interactor
                .cmd(&format!("sudo cat {} 2>/dev/null || true", log_file))
                .map(|o| o.stdout)
                .unwrap_or_default();

            let extra_logs = debug_get_postgres_logs(interactor, pg_version);
            if !extra_logs.is_empty() {
                log.push_str(&extra_logs);
            }

            anyhow::bail!("PostgreSQL did not become ready after PITR:\n{}", log);
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
