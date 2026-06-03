use crate::{
    helper::{base64::base64_encode, config::config_get_nodes},
    postgres_unit::{
        entity::BackupMetadata,
        helper::{cmdw, connect_to_node, pg_wait_all_replicas},
    },
    s3::S3Client,
    server_interactor::server_interactor_trait::ServerInteractor,
};

pub async fn postgres_restore(
    config: &crate::config::Config,
    primary_node: &crate::config::NodeConfig,
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

    // let pg_ctl = format!("/usr/lib/postgresql/{}/bin/pg_ctl", pg_version);

    let pg_combinebackup = format!("/usr/lib/postgresql/{}/bin/pg_combinebackup", pg_version);
    let pg_verifybackup = format!("/usr/lib/postgresql/{}/bin/pg_verifybackup", pg_version);
    let pgdata_dir = format!("/var/lib/postgresql/{}/main", pg_version);

    // Gather all PostgreSQL nodes
    let pg_nodes = config_get_nodes(&config, "postgres");

    // 1. Stop all Patroni and PostgreSQL on all nodes
    let mut handles = vec![];
    for node in &pg_nodes {
        let node = node.clone();
        let config = config.clone();

        let handle = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            println!(
                "Stopping Patroni/PostgreSQL on node {} for restore",
                node.name
            );

            match connect_to_node(&node, &config) {
                Ok(interactor) => {
                    let _ = interactor.stop_service("patroni");
                    let _ = interactor.stop_service("postgresql --no-block");
                    let _ = interactor.cmd("sudo pkill -9 -u postgres postgres");
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
    println!("Clearing DCS cluster state...");
    let _ = interactor.cmd("sudo env ETCDCTL_API=3 etcdctl del /service/postgres-cluster --prefix");

    // 3. Clear existing data directory on all nodes
    let mut handles = vec![];
    for node in &pg_nodes {
        let node = node.clone();
        let config = config.clone();
        let pgdata_dir = pgdata_dir.clone();

        let handle = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            println!("\tClearing postgres data directory on node {}", node.name);

            match connect_to_node(&node, &config) {
                Ok(interactor) => {
                    let _ = interactor.cmd(&format!("sudo rm -rf {}", pgdata_dir));

                    cmdw(
                        &*interactor,
                        &format!("sudo -u postgres mkdir -p {}", pgdata_dir),
                    )?;

                    cmdw(&*interactor, &format!("sudo chmod 700 {}", pgdata_dir))?;
                    println!("\tCleared postgres data directory on node {}", node.name);
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

    // 6. Set ownership
    cmdw(
        interactor,
        &format!("sudo chown -R postgres:postgres {}", pgdata_dir),
    )?;
    cmdw(interactor, &format!("sudo chmod 700 {}", pgdata_dir))?;

    // Remove old signals and dynamic JSON on primary node
    let _ = interactor.cmd(&format!(
        "sudo -u postgres rm -f {}/recovery.signal {}/standby.signal {}/patroni.dynamic.json",
        pgdata_dir, pgdata_dir, pgdata_dir
    ));

    let pg_ctl = format!("/usr/lib/postgresql/{}/bin/pg_ctl", pg_version);

    if let Some(target_time) = pitr_time {
        // Write PITR settings to postgresql.auto.conf in pgdata_dir
        let pitr_conf_path = format!("{}/postgresql.auto.conf", pgdata_dir);
        let pitr_conf_content = format!(
            "restore_command = 'cp /var/lib/postgresql/wal_archive/%f %p'\nrecovery_target_time = '{}'\nrecovery_target_action = promote\nrecovery_target_inclusive = on\nrecovery_target_timeline = 'current'\n",
            target_time
        );
        let b64 = base64_encode(&pitr_conf_content);

        cmdw(
            interactor,
            &format!(
                "echo {} | base64 -d | sudo -u postgres tee -a {} > /dev/null",
                b64, pitr_conf_path
            ),
        )?;

        // Create recovery.signal (PG12+ triggers archive recovery mode)
        cmdw(
            interactor,
            &format!("sudo -u postgres touch {}/recovery.signal", pgdata_dir),
        )?;

        // Start PostgreSQL directly using pg_ctl to perform the archive recovery (PITR).
        // This prevents Patroni from overwriting our recovery configuration on startup.
        // We use -l /tmp/pg_start.log to prevent open stdout/stderr from hanging the SSH session.
        println!("Performing Point-in-Time Recovery via direct PostgreSQL start...");
        let pg_start_out = interactor.cmd(&format!(
            "sudo -u postgres {} -D {} -l /tmp/pg_start.log start",
            pg_ctl, pgdata_dir
        ))?;
        if pg_start_out.exit_code != 0 {
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
            let check_res =
                interactor.cmd("sudo -u postgres psql -t -A -c \"SELECT pg_is_in_recovery();\"");
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
        let _ = interactor.cmd("sudo rm -f /tmp/pg_start.log");

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
    println!("Waiting for primary node to become Patroni leader...");
    let mut primary_ready = false;
    let check_leader_cmd = "curl -s -o /dev/null -w \"%{http_code}\" http://127.0.0.1:8008/primary";
    let start_time = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(300); // 5 minutes

    while start_time.elapsed() < timeout {
        if let Ok(out) = interactor.cmd(check_leader_cmd) {
            if out.stdout.trim() == "200" {
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

        anyhow::bail!(
            "Timeout waiting for primary node to become Patroni leader. Logs:\n\n{}",
            logs
        );
    }
    println!(
        "Primary node {} is now the Patroni leader.",
        primary_node.name
    );

    if pitr_time.is_some() {
        // Clean up PITR settings from postgresql.auto.conf on primary
        let pitr_conf_path = format!("{}/postgresql.auto.conf", pgdata_dir);
        let _ = interactor.cmd(&format!(
            "sudo -u postgres sed -i '/restore_command/d;/recovery_target/d' {}",
            pitr_conf_path
        ));
        let _ = interactor.cmd("sudo -u postgres psql -c \"SELECT pg_reload_conf();\"");
    }

    // Start Patroni on replica nodes
    let mut handles = vec![];
    for node in &pg_nodes {
        if node.internal_ip == primary_node.internal_ip {
            continue;
        }
        let node = node.clone();
        let config = config.clone();

        let handle = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            println!("Starting Patroni on replica node {}...", node.name);
            let node_interactor = connect_to_node(&node, &config)?;
            node_interactor.restart_service("patroni")?;
            Ok(())
        });

        handles.push(handle);
    }
    for handle in handles {
        handle.await??;
    }

    // Wait for all replica nodes to join and reach "running" state
    println!("Waiting for replica nodes to join the cluster...");
    pg_wait_all_replicas(interactor, &pg_nodes);

    Ok(())
}
