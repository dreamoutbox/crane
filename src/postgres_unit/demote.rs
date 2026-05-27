use crate::{config, postgres_unit::helper::connect_to_node};

pub fn run_demote_node(
    node: &config::NodeConfig,
    leader: &config::NodeConfig,
    pg_version: &str,
    replica_pass: &str,
    config: &config::Config,
) -> anyhow::Result<()> {
    let interactor = connect_to_node(node, config)?;

    let app_node_ips: Vec<String> = config
        .nodes
        .iter()
        .filter(|n| n.roles.contains(&"app".to_string()))
        .map(|n| n.internal_ip.clone())
        .collect();

    crate::postgres_unit::setup::setup_postgres_follower(
        &*interactor,
        pg_version,
        &leader.internal_ip,
        "replicator",
        replica_pass,
        &app_node_ips,
    )
}
