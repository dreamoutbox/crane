use crate::config;
use crate::postgres_unit::entity::BackupRegistry;
use crate::postgres_unit::entity::PostgresNode;
use crate::postgres_unit::helper::connect_to_node;
use crate::postgres_unit::helper::find_node_config_with_fallback;
use crate::postgres_unit::helper::{get_pg_version, get_replica_pass};
use crate::postgres_unit::tasks::*;
use crate::s3::get_s3_config;
use crate::s3::s3_client::{RealS3Client, S3Client};
use crate::server_interactor::server_interactor_trait::ServerInteractor;
use crate::ssh::SSHSession;
use std::path::Path;

pub fn promote(
    config_path: &Path,
    target_node_str: &str,
    get_interactor: fn(SSHSession) -> anyhow::Result<Box<dyn ServerInteractor>>,
) -> anyhow::Result<()> {
    let config = config::load_config(config_path)?;
    let config_dir = config_path.parent().unwrap_or(Path::new("."));
    let env_path = config_dir.join(".env");
    let dot_env = config::load_env_file(&env_path).unwrap_or_default();

    let target_conf = find_node_config_with_fallback(target_node_str, &config, get_interactor)
        .ok_or_else(|| anyhow::anyhow!("Node '{}' not found in configuration", target_node_str))?;

    if !target_conf.roles.contains(&"postgres".to_string()) {
        anyhow::bail!(
            "Node '{}' does not have the 'postgres' role",
            target_node_str
        );
    }

    let pg_version = config
        .db
        .as_ref()
        .and_then(|db| db.postgres.as_ref())
        .and_then(|pg| pg.get("version"))
        .and_then(|val| val.as_str())
        .unwrap_or("16")
        .to_string();

    let replica_pass = dot_env
        .get("POSTGRES_PASSWORD")
        .cloned()
        .unwrap_or_else(|| "repl_password".to_string());

    let current_leader = postgres_get_leader(&config, get_interactor)?;

    if let Some(ref leader) = current_leader {
        if leader.internal_ip == target_conf.internal_ip {
            println!(
                "Node '{}' is already the active PostgreSQL leader.",
                target_node_str
            );
            return Ok(());
        }

        // Safe promotion sequence:
        // 1. Demote the target follower node first to follow the current leader (synchronize fully)
        println!(
            "\nSynchronizing target follower node {} with current leader {} before promotion...",
            target_conf.name, leader.name
        );
        run_demote_node(
            &target_conf,
            leader,
            &pg_version,
            &replica_pass,
            &config,
            get_interactor,
        )?;
    }

    // Configure target node's primary rules before promotion
    println!(
        "\nConfiguring replication and local trust rules on node {}...",
        target_conf.name
    );
    let target_interactor = connect_to_node(&target_conf, &config, get_interactor)?;

    let target_follower_ips: Vec<String> = config
        .nodes
        .iter()
        .filter(|n| n.roles.contains(&"postgres".to_string()))
        .filter(|n| n.internal_ip != target_conf.internal_ip)
        .map(|n| n.internal_ip.clone())
        .collect();
    let app_node_ips: Vec<String> = config
        .nodes
        .iter()
        .filter(|n| n.roles.contains(&"app".to_string()))
        .map(|n| n.internal_ip.clone())
        .collect();

    crate::postgres_unit::setup::configure_postgres_primary_rules(
        &*target_interactor,
        &pg_version,
        "replicator",
        &target_follower_ips,
        &app_node_ips,
    )?;

    // 2. Promote the target node to leader
    println!(
        "\nPromoting node {} to PostgreSQL leader...",
        target_conf.name
    );
    let pg_ctl = format!("/usr/lib/postgresql/{}/bin/pg_ctl", pg_version);
    let promote_cmd = format!(
        "sudo -u postgres {} -D /var/lib/postgresql/{}/main promote",
        pg_ctl, pg_version
    );
    target_interactor.cmd(&promote_cmd)?;

    // 3. Demote all other nodes to follow the new leader
    let pg_nodes: Vec<_> = config
        .nodes
        .iter()
        .filter(|n| n.roles.contains(&"postgres".to_string()))
        .cloned()
        .collect();

    for node in pg_nodes {
        if node.internal_ip != target_conf.internal_ip {
            println!(
                "\nDemoting node {} to follow new leader {}...",
                node.name, target_conf.name
            );
            run_demote_node(
                &node,
                &target_conf,
                &pg_version,
                &replica_pass,
                &config,
                get_interactor,
            )?;
        }
    }

    // 4. Update HAProxy configs on all app nodes
    let app_nodes: Vec<_> = config
        .nodes
        .iter()
        .filter(|n| n.roles.contains(&"app".to_string()))
        .cloned()
        .collect();

    let follower_ips: Vec<String> = config
        .nodes
        .iter()
        .filter(|n| n.roles.contains(&"postgres".to_string()))
        .filter(|n| n.internal_ip != target_conf.internal_ip)
        .map(|n| n.internal_ip.clone())
        .collect();

    for app_node in &app_nodes {
        println!(
            "\nUpdating HAProxy configuration on app node {}...",
            app_node.name
        );
        let app_interactor = connect_to_node(app_node, &config, get_interactor)?;

        crate::postgres_unit::haproxy::setup_haproxy(
            &*app_interactor,
            &target_conf.internal_ip,
            &follower_ips,
        )?;
    }

    println!(
        "\nPROMOTION TO LEADER COMPLETE FOR NODE '{}'",
        target_conf.name
    );
    Ok(())
}

pub fn demote(
    config_path: &Path,
    target_node_str: &str,
    get_interactor: fn(SSHSession) -> anyhow::Result<Box<dyn ServerInteractor>>,
) -> anyhow::Result<()> {
    let config = config::load_config(config_path)?;
    let config_dir = config_path.parent().unwrap_or(Path::new("."));
    let env_path = config_dir.join(".env");
    let dot_env = config::load_env_file(&env_path).unwrap_or_default();

    let target_conf = find_node_config_with_fallback(target_node_str, &config, get_interactor)
        .ok_or_else(|| anyhow::anyhow!("Node '{}' not found in configuration", target_node_str))?;

    if !target_conf.roles.contains(&"postgres".to_string()) {
        anyhow::bail!(
            "Node '{}' does not have the 'postgres' role",
            target_node_str
        );
    }

    let pg_version = config
        .db
        .as_ref()
        .and_then(|db| db.postgres.as_ref())
        .and_then(|pg| pg.get("version"))
        .and_then(|val| val.as_str())
        .unwrap_or("16")
        .to_string();

    let replica_pass = dot_env
        .get("POSTGRES_PASSWORD")
        .cloned()
        .unwrap_or_else(|| "repl_password".to_string());

    let current_leader = postgres_get_leader(&config, get_interactor)?;

    let leader = current_leader.ok_or_else(|| {
        anyhow::anyhow!("Cannot demote: No active PostgreSQL leader discovered in the cluster to replicate from.")
    })?;

    if leader.internal_ip == target_conf.internal_ip {
        anyhow::bail!(
            "Node '{}' is currently the active leader. Demoting the leader directly is not permitted; please promote another node to leader instead.",
            target_node_str
        );
    }

    println!(
        "\nDemoting node {} to follow leader {}...",
        target_conf.name, leader.name
    );

    run_demote_node(
        &target_conf,
        &leader,
        &pg_version,
        &replica_pass,
        &config,
        get_interactor,
    )?;

    println!("\nDEMOTION COMPLETE FOR NODE '{}'", target_conf.name);
    Ok(())
}

pub fn status(
    config_path: &Path,
    get_interactor: fn(SSHSession) -> anyhow::Result<Box<dyn ServerInteractor>>,
) -> anyhow::Result<()> {
    let config = config::load_config(config_path)?;

    let pg_nodes: Vec<_> = config
        .nodes
        .iter()
        .filter(|n| n.roles.contains(&"postgres".to_string()))
        .cloned()
        .collect();

    let pg_version_config = config
        .db
        .as_ref()
        .and_then(|db| db.postgres.as_ref())
        .and_then(|pg| pg.get("version"))
        .and_then(|val| val.as_str())
        .unwrap_or("16")
        .to_string();

    let mut statuses = Vec::new();
    let mut primary_host = "Unknown".to_string();

    for node in &pg_nodes {
        let address = format!("{}:{}", node.public_ip, node.port);
        let mut hostname = node.host.clone();
        let mut role = "Unknown".to_string();
        let mut version = pg_version_config.clone();
        let mut health = "Unhealthy".to_string();

        match connect_to_node(node, &config, get_interactor) {
            Ok(interactor) => {
                // 1. Get Hostname
                if let Ok(h) = interactor.cmd("hostname") {
                    let h_trimmed = h.stdout.trim();
                    if !h_trimmed.is_empty() {
                        hostname = h_trimmed.to_string();
                    }
                }

                // 2. Check Recovery & DB Version
                let recovery_cmd =
                    r#"sudo -u postgres psql -t -A -c "select pg_is_in_recovery();""#;
                let version_cmd = r#"sudo -u postgres psql -t -A -c "show server_version;""#;

                let is_recovery = interactor.cmd(recovery_cmd);
                let db_ver_str = interactor.cmd(version_cmd);

                if let Ok(rec) = is_recovery {
                    let rec_trimmed = rec.stdout.trim();
                    if rec_trimmed == "f" {
                        role = "Leader".to_string();
                        primary_host = hostname.clone();
                        health = "Healthy".to_string();
                    } else if rec_trimmed == "t" {
                        role = "Follower".to_string();
                        health = "Healthy".to_string();
                    }
                }

                if let Ok(v_str) = db_ver_str {
                    let v_trimmed = v_str.stdout.trim();
                    if let Some(major) = v_trimmed.split('.').next() {
                        let major_clean = major.trim();
                        if !major_clean.is_empty() {
                            version = major_clean.to_string();
                        }
                    }
                }
            }

            Err(_) => {
                // SSH connection failure defaults to Unhealthy
            }
        }

        statuses.push(PostgresNode {
            hostname,
            address,
            role,
            version,
            health,
        });
    }

    // Identify backups (all postgres nodes that are not the leader)
    let mut backups = Vec::new();
    for status in &statuses {
        if status.hostname != primary_host {
            backups.push(format!("{}:5000", status.hostname));
        }
    }

    // Print expected output format
    println!("\nHAProxy");
    println!("Primary: {}:5000", primary_host);
    println!("Backup: {}", backups.join(","));

    for status in &statuses {
        println!("\n{}", status.hostname);
        println!("Address: {}", status.address);
        println!("Role: {}", status.role);
        println!("DB version: {}", status.version);
        println!("Health: {}", status.health);
    }
    println!();

    Ok(())
}

pub fn backup(
    config_path: &Path,
    backup_type: &str,
    get_interactor: fn(SSHSession) -> anyhow::Result<Box<dyn ServerInteractor>>,
) -> anyhow::Result<()> {
    let config = config::load_config(config_path)?;
    let config_dir = config_path.parent().unwrap_or(Path::new("."));
    let env_path = config_dir.join(".env");
    let dot_env = config::load_env_file(&env_path).unwrap_or_default();

    let s3_config = get_s3_config(&config, &dot_env)?;
    let primary_node = postgres_get_leader(&config, get_interactor)?
        .ok_or_else(|| anyhow::anyhow!("No active PostgreSQL leader found in the cluster."))?;

    let pg_version = get_pg_version(&config);
    let replica_pass = get_replica_pass(&dot_env);

    let s3_client = RealS3Client::new(&s3_config)?;
    let interactor = connect_to_node(&primary_node, &config, get_interactor)?;

    let registry_key = "backups/registry.toml";
    let registry = match s3_client.get_object(registry_key) {
        Ok(data) => {
            let content = String::from_utf8_lossy(&data).to_string();
            toml::from_str::<BackupRegistry>(&content)
                .expect("Failed to parse backups/registry.toml")
        }
        Err(_) => BackupRegistry::default(),
    };
    let last_backup = registry.backups.last();

    println!(
        "Starting PostgreSQL {} backup...",
        backup_type.to_uppercase()
    );
    let meta = crate::postgres_unit::backup::run_backup(
        &*interactor,
        &s3_client,
        &pg_version,
        backup_type,
        &replica_pass,
        &s3_config.bucket,
        last_backup,
    )?;

    println!("\nBackup successful!");
    println!("{}", meta.id);
    println!("Date: {}", meta.date);
    println!("Time: {}", meta.time);
    println!("Type: {}", meta.backup_type);
    println!("LOCAL: {}", meta.local_path);
    println!("S3: {}", meta.s3_path);

    Ok(())
}

pub fn list_backups(
    config_path: &Path,
    _get_interactor: fn(SSHSession) -> anyhow::Result<Box<dyn ServerInteractor>>,
) -> anyhow::Result<()> {
    let config = config::load_config(config_path)?;
    let config_dir = config_path.parent().unwrap_or(Path::new("."));
    let env_path = config_dir.join(".env");
    let dot_env = config::load_env_file(&env_path).unwrap_or_default();

    let s3_config = get_s3_config(&config, &dot_env)?;
    let s3_client = RealS3Client::new(&s3_config)?;

    let registry_key = "backups/registry.toml";
    let registry = match s3_client.get_object(registry_key) {
        Ok(data) => {
            let content = String::from_utf8_lossy(&data).to_string();
            toml::from_str::<BackupRegistry>(&content).unwrap_or_default()
        }
        Err(_) => {
            println!("No backups found in cluster.");
            return Ok(());
        }
    };

    if registry.backups.is_empty() {
        println!("No backups found in cluster.");
        return Ok(());
    }

    for (idx, backup) in registry.backups.iter().enumerate() {
        if idx > 0 {
            println!();
        }

        println!("{}", backup.id);
        println!("Date: {}", backup.date);
        println!("Time: {}", backup.time);
        println!("Type: {}", backup.backup_type);
        println!("LOCAL: {}", backup.local_path);
        println!("S3: {}", backup.s3_path);
    }

    Ok(())
}

pub fn restore(
    config_path: &Path,
    target_id: &str,
    get_interactor: fn(SSHSession) -> anyhow::Result<Box<dyn ServerInteractor>>,
) -> anyhow::Result<()> {
    let config = config::load_config(config_path)?;
    let config_dir = config_path.parent().unwrap_or(Path::new("."));
    let env_path = config_dir.join(".env");
    let dot_env = config::load_env_file(&env_path).unwrap_or_default();

    let s3_config = get_s3_config(&config, &dot_env)?;
    let primary_node = match postgres_get_leader(&config, get_interactor)? {
        Some(node) => node,
        None => config
            .nodes
            .iter()
            .find(|n| n.roles.contains(&"postgres".to_string()))
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("No PostgreSQL nodes found in configuration"))?,
    };

    let pg_version = get_pg_version(&config);
    let s3_client = RealS3Client::new(&s3_config)?;

    let registry_key = "backups/registry.toml";
    let registry_data = s3_client
        .get_object(registry_key)
        .map_err(|e| anyhow::anyhow!("Failed to download backup registry from S3: {}", e))?;
    let content = String::from_utf8_lossy(&registry_data).to_string();
    let registry = toml::from_str::<BackupRegistry>(&content).unwrap_or_default();

    let target_backup = registry
        .backups
        .iter()
        .find(|b| b.id == target_id)
        .ok_or_else(|| anyhow::anyhow!("Backup ID '{}' not found in registry", target_id))?;

    let mut chain = Vec::new();
    let mut current = target_backup.clone();
    chain.push(current.clone());

    while let Some(ref base_id) = current.base {
        let parent = registry
            .backups
            .iter()
            .find(|b| &b.id == base_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Broken backup chain: parent backup ID '{}' not found in registry",
                    base_id
                )
            })?;
        chain.push(parent.clone());
        current = parent.clone();
    }

    chain.reverse();

    let interactor = connect_to_node(&primary_node, &config, get_interactor)?;

    println!("Restoring database to backup ID: {}...", target_id);
    crate::postgres_unit::backup::run_restore(
        &*interactor,
        &s3_client,
        &pg_version,
        target_backup,
        &chain,
    )?;

    println!("Restore complete! PostgreSQL is online.");
    Ok(())
}

pub fn logs(
    config_path: &Path,
    target_node_str: &str,
    get_interactor: fn(SSHSession) -> anyhow::Result<Box<dyn ServerInteractor>>,
) -> anyhow::Result<()> {
    let config = config::load_config(config_path)?;
    let target_conf = find_node_config_with_fallback(target_node_str, &config, get_interactor)
        .ok_or_else(|| anyhow::anyhow!("Node '{}' not found in configuration", target_node_str))?;

    if !target_conf.roles.contains(&"postgres".to_string()) {
        anyhow::bail!(
            "Node '{}' does not have the 'postgres' role",
            target_node_str
        );
    }

    let pg_version = config
        .db
        .as_ref()
        .and_then(|db| db.postgres.as_ref())
        .and_then(|pg| pg.get("version"))
        .and_then(|val| val.as_str())
        .unwrap_or("16")
        .to_string();

    let interactor = connect_to_node(&target_conf, &config, get_interactor)?;

    let log_path = format!("/var/log/postgresql/postgresql-{}-main.log", pg_version);
    let cmd = format!("sudo tail -n 100 {}", log_path);
    let output = interactor.cmd(&cmd)?;
    print!("{}\n", output.stdout);

    Ok(())
}
