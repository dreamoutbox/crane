use crate::config;
use crate::server_interactor::server_interactor_trait::ServerInteractor;
use crate::ssh::SSHSession;
use std::path::Path;

fn find_node_config<'a>(
    target: &str,
    config: &'a config::Config,
) -> Option<&'a config::NodeConfig> {
    config
        .nodes
        .iter()
        .find(|n| n.host == target || n.internal_ip == target || n.public_ip == target)
}

fn connect_to_node(
    node: &config::NodeConfig,
    config: &config::Config,
    get_interactor: fn(SSHSession) -> anyhow::Result<Box<dyn ServerInteractor>>,
) -> anyhow::Result<Box<dyn ServerInteractor>> {
    let private_key = crate::helper::keys::find_private_key_for_user(&node.user, config);
    let private_key = if private_key.is_empty() {
        crate::helper::keys::get_any_private_key(config)
    } else {
        private_key
    };
    let ssh = SSHSession::new(
        node.host.clone(),
        node.user.clone(),
        private_key,
        Some(node.port),
    );
    get_interactor(ssh)
}

fn discover_active_leader(
    config: &config::Config,
    get_interactor: fn(SSHSession) -> anyhow::Result<Box<dyn ServerInteractor>>,
) -> anyhow::Result<Option<config::NodeConfig>> {
    let pg_nodes: Vec<_> = config
        .nodes
        .iter()
        .filter(|n| n.roles.contains(&"postgres".to_string()))
        .cloned()
        .collect();

    for node in pg_nodes {
        if let Ok(interactor) = connect_to_node(&node, config, get_interactor) {
            let cmd = r#"sudo -u postgres psql -t -A -c "select pg_is_in_recovery();""#;
            if let Ok(output) = interactor.cmd(cmd) {
                if output.trim() == "f" {
                    return Ok(Some(node));
                }
            }
        }
    }
    Ok(None)
}

fn run_demote_node(
    node: &config::NodeConfig,
    leader: &config::NodeConfig,
    pg_version: &str,
    replica_pass: &str,
    config: &config::Config,
    get_interactor: fn(SSHSession) -> anyhow::Result<Box<dyn ServerInteractor>>,
) -> anyhow::Result<()> {
    let interactor = connect_to_node(node, config, get_interactor)?;
    crate::postgres_unit::setup::setup_postgres_follower(
        &*interactor,
        pg_version,
        &leader.internal_ip,
        "replicator",
        replica_pass,
    )
}

pub fn promote(
    config_path: &Path,
    target_node_str: &str,
    get_interactor: fn(SSHSession) -> anyhow::Result<Box<dyn ServerInteractor>>,
) -> anyhow::Result<()> {
    let config = config::load_config(config_path)?;
    let config_dir = config_path.parent().unwrap_or(Path::new("."));
    let env_path = config_dir.join(".env");
    let dot_env = config::load_env_file(&env_path).unwrap_or_default();

    let target_conf = find_node_config(target_node_str, &config)
        .ok_or_else(|| anyhow::anyhow!("Node '{}' not found in configuration", target_node_str))?
        .clone();

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

    let current_leader = discover_active_leader(&config, get_interactor)?;

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
            target_conf.host, leader.host
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
        target_conf.host
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
        target_conf.host
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
                node.host, target_conf.host
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
            app_node.host
        );
        let app_interactor = connect_to_node(app_node, &config, get_interactor)?;
        crate::postgres_unit::setup::setup_haproxy(
            &*app_interactor,
            &target_conf.internal_ip,
            &follower_ips,
        )?;
    }

    println!(
        "\nPROMOTION TO LEADER COMPLETE FOR NODE '{}'",
        target_conf.host
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

    let target_conf = find_node_config(target_node_str, &config)
        .ok_or_else(|| anyhow::anyhow!("Node '{}' not found in configuration", target_node_str))?
        .clone();

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

    let current_leader = discover_active_leader(&config, get_interactor)?;

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
        target_conf.host, leader.host
    );
    run_demote_node(
        &target_conf,
        &leader,
        &pg_version,
        &replica_pass,
        &config,
        get_interactor,
    )?;

    println!("\nDEMOTION COMPLETE FOR NODE '{}'", target_conf.host);
    Ok(())
}
