// ======================
// OBSOLETE: no longer useful
// ======================
//
// use crate::postgres_unit::helper::{
//     connect_to_node, find_node_config_with_fallback, postgres_get_primary,
// };

// pub fn run_demote_cmd(config: &crate::config::Config, target_node_str: &str) -> anyhow::Result<()> {
//     let target_conf = find_node_config_with_fallback(target_node_str, &config)
//         .ok_or_else(|| anyhow::anyhow!("Node '{}' not found in configuration", target_node_str))?;

//     if !target_conf.roles.contains(&"postgres".to_string()) {
//         anyhow::bail!(
//             "Node '{}' does not have the 'postgres' role",
//             target_node_str
//         );
//     }

//     let current_leader = postgres_get_primary(&config)?;

//     let leader = current_leader.ok_or_else(|| {
//         anyhow::anyhow!("Cannot demote: No active PostgreSQL leader discovered in the cluster to replicate from.")
//     })?;

//     if leader.internal_ip == target_conf.internal_ip {
//         anyhow::bail!(
//             "Node '{}' is currently the active leader. Demoting the leader directly is not permitted; please promote another node to leader instead.",
//             target_node_str
//         );
//     }

//     // Under Patroni, standbys follow the leader automatically. We can reinit the node if we want to force it to follow the leader cleanly.
//     println!(
//         "\nReinitializing Patroni standby node {} to ensure it follows leader {}...",
//         target_conf.name, leader.name
//     );
//     let target_interactor = connect_to_node(&target_conf, &config)?;
//     let reinit_cmd = format!(
//         "sudo patronictl -c /etc/patroni/config.yml reinit postgres-cluster {} --force",
//         target_conf.name
//     );
//     target_interactor.cmd(&reinit_cmd)?;

//     println!("\nDEMOTION COMPLETE FOR NODE '{}'", target_conf.name);
//     Ok(())
// }
