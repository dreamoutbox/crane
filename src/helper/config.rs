use crate::config;

pub fn config_get_nodes(config: &config::Config, role: &str) -> Vec<config::NodeConfig> {
    let nodes: Vec<_> = config
        .nodes
        .iter()
        .filter(|n| n.roles.contains(&role.to_string()))
        .cloned()
        .collect();

    nodes
}
