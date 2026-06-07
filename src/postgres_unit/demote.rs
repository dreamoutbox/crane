// ====================================
// OBSOLETE: no longer useful
// ====================================
// use crate::{config, server_interactor::get_server_interactor};
//
// pub fn run_demote_node(
//     node: &config::NodeConfig,
//     _leader: &config::NodeConfig,
//     _pg_version: &str,
//     _replica_pass: &str,
// ) -> anyhow::Result<()> {
//     let interactor = get_server_interactor(&node.name)?;
//     let reinit_cmd = format!(
//         "sudo patronictl -c /etc/patroni/config.yml reinit postgres-cluster {} --force",
//         node.name
//     );
//     interactor.cmd(&reinit_cmd)?;
//     Ok(())
// }
