use crate::{config, postgres_unit::helper::connect_to_node};

pub fn run_demote_node(
    node: &config::NodeConfig,
    _leader: &config::NodeConfig,
    _pg_version: &str,
    _replica_pass: &str,
    config: &config::Config,
) -> anyhow::Result<()> {
    let interactor = connect_to_node(node, config)?;
    let reinit_cmd = format!(
        "sudo patronictl -c /etc/patroni/config.yml reinit postgres-cluster {} --force",
        node.name
    );
    interactor.cmd(&reinit_cmd)?;
    Ok(())
}
