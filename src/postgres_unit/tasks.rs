use crate::{
    config, server_interactor::server_interactor_trait::ServerInteractor, ssh::SSHSession,
};

pub fn find_node_config<'a>(
    target: &str,
    config: &'a config::Config,
) -> Option<&'a config::NodeConfig> {
    config.nodes.iter().find(|n| {
        n.host == target || n.internal_ip == target || n.public_ip == target || n.name == target
    })
}

pub fn connect_to_node(
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

pub fn find_node_config_with_fallback(
    target: &str,
    config: &config::Config,
    get_interactor: fn(SSHSession) -> anyhow::Result<Box<dyn ServerInteractor>>,
) -> Option<config::NodeConfig> {
    if let Some(n) = find_node_config(target, config) {
        return Some(n.clone());
    }

    // Fallback: connect to pg nodes and check their hostname
    let pg_nodes: Vec<_> = config
        .nodes
        .iter()
        .filter(|n| n.roles.contains(&"postgres".to_string()))
        .cloned()
        .collect();

    for node in pg_nodes {
        if let Ok(interactor) = connect_to_node(&node, config, get_interactor) {
            if let Ok(h) = interactor.cmd("hostname") {
                if h.stdout.trim() == target {
                    return Some(node);
                }
            }
        }
    }

    None
}

pub fn postgres_get_leader(
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
                if output.stdout.trim() == "f" {
                    return Ok(Some(node));
                }
            }
        }
    }
    Ok(None)
}

pub fn run_demote_node(
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
