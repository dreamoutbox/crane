use crate::{
    config, postgres_unit::helper::connect_to_node,
    server_interactor::server_interactor_trait::ServerInteractor, ssh::SSHSession,
};

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
