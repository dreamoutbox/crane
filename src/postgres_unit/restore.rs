use crate::{
    helper::base64::base64_encode,
    postgres_unit::{
        entity::BackupMetadata,
        helper::{cmdw, connect_to_node},
    },
    s3::s3_client::S3Client,
    server_interactor::server_interactor_trait::ServerInteractor,
};

pub fn postgres_restore(
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
    let pg_nodes: Vec<_> = config
        .nodes
        .iter()
        .filter(|n| n.roles.contains(&"postgres".to_string()))
        .cloned()
        .collect();

    // 1. Stop Patroni and PostgreSQL on all nodes
    for node in &pg_nodes {
        println!(
            "Stopping Patroni/PostgreSQL on node {} for restore",
            node.name
        );

        let node_interactor = if node.internal_ip == primary_node.internal_ip {
            None
        } else {
            match connect_to_node(node, config) {
                Ok(int) => Some(int),
                Err(e) => {
                    println!("Warning: failed to connect to node {}: {}", node.name, e);
                    None
                }
            }
        };

        let interactor = node_interactor.as_deref().unwrap_or(interactor);
        let _ = interactor.cmd("sudo systemctl stop patroni");
        let _ = interactor.cmd("sudo systemctl stop postgresql --no-block");
        // let _ = int.cmd("sudo pkill -f patroni");

        // let _ = int.cmd(&format!(
        //     "sudo systemctl stop postgresql@{}-main --no-block",
        //     pg_version
        // ));

        // let _ = int.cmd(&format!(
        //     "sudo -u postgres {} -D {} stop -m immediate",
        //     pg_ctl, pgdata_dir
        // ));

        // let _ = int.cmd("sudo pkill -u postgres -f postgres");
    }

    // 2. Clear DCS (etcd) keys for the cluster to prevent conflicts
    println!("Clearing DCS cluster state...");
    let _ = interactor.cmd("sudo env ETCDCTL_API=3 etcdctl del /service/postgres-cluster --prefix");

    // 3. Clear existing data directory on all nodes
    for node in &pg_nodes {
        println!("Clearing data directory on node {}...", node.name);
        let node_interactor = if node.internal_ip == primary_node.internal_ip {
            None
        } else {
            match connect_to_node(node, config) {
                Ok(int) => Some(int),
                Err(e) => {
                    println!("Warning: failed to connect to node {}: {}", node.name, e);
                    None
                }
            }
        };

        let interactor = node_interactor.as_deref().unwrap_or(interactor);

        let _ = interactor.cmd(&format!("sudo rm -rf {}", pgdata_dir));
        cmdw(
            interactor,
            &format!("sudo -u postgres mkdir -p {}", pgdata_dir),
        )?;
        cmdw(interactor, &format!("sudo chmod 700 {}", pgdata_dir))?;
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

    // 6. Set ownership and start service
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
    }

    // Start Patroni on the primary node only
    println!("Starting Patroni on primary node {}...", primary_node.name);
    cmdw(interactor, "sudo systemctl restart patroni")?;

    // Wait for primary node to become the Patroni leader
    println!("Waiting for primary node to become Patroni leader...");
    let mut primary_ready = false;
    let check_leader_cmd = "curl -s -o /dev/null -w \"%{http_code}\" http://localhost:8008/primary";
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
            "Timeout waiting for primary node to become Patroni leader. Logs:\n{}",
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
    for node in &pg_nodes {
        if node.internal_ip == primary_node.internal_ip {
            continue;
        }

        println!("Starting Patroni on replica node {}...", node.name);
        let node_interactor = connect_to_node(node, config)?;
        cmdw(&*node_interactor, "sudo systemctl restart patroni")?;
    }

    // Wait for all replica nodes to join and reach "running" state
    if pg_nodes.len() > 1 {
        println!("Waiting for replica nodes to join the cluster...");
        let replica_start_time = std::time::Instant::now();
        let replica_timeout = std::time::Duration::from_secs(30);

        let list_cmd = "sudo -u postgres patronictl -c /etc/patroni/config.yml list";

        while replica_start_time.elapsed() < replica_timeout {
            if let Ok(out) = interactor.cmd(list_cmd) {
                let output = out.stdout;
                let mut all_running = true;
                for node in &pg_nodes {
                    let node_line = output.lines().find(|l| l.contains(&node.name));
                    dbg!(&node_line);

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
                    break;
                }
            }

            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }

    Ok(())
}
