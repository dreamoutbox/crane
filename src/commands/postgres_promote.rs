use crate::config::find_node_config;
use crate::postgres_unit::helper::pg_get_primary;
use crate::server_interactor::get_server_interactor;

pub fn run_postgres_promote_cmd(
    config: &crate::config::Config,
    target_node: &str,
) -> anyhow::Result<()> {
    let target_conf_node = find_node_config(&config, target_node)
        .ok_or_else(|| anyhow::anyhow!("Node '{}' not found in configuration", target_node))?;

    if !target_conf_node.roles.contains(&"postgres".to_string()) {
        anyhow::bail!("Node '{}' does not have the 'postgres' role", target_node);
    }

    let current_leader = pg_get_primary(&config)?;

    if let Some(ref leader) = current_leader {
        if leader.internal_ip == target_conf_node.internal_ip {
            println!(
                "Node '{}' is already the active PostgreSQL leader.",
                target_node
            );
            return Ok(());
        }

        // Trigger switchover using patronictl
        println!(
            "\nSwitching over PostgreSQL leader from {} to {} using Patroni...",
            leader.name, target_conf_node.name
        );
        let target_interactor = get_server_interactor(&target_conf_node.name)?;
        let switch_cmd = format!(
            "sudo patronictl -c /etc/patroni/config.yml switchover --leader {} --candidate {} --scheduled now --force",
            leader.name, target_conf_node.name
        );
        let switch_output = target_interactor.cmd(&switch_cmd)?;

        if switch_output.exit_code != 0 {
            println!(
                "\nFailed to switchover PostgreSQL leader from {} to {}",
                leader.name, target_conf_node.name
            );

            println!("\nSTDOUT: {}", switch_output.stdout);
            println!("\nSTDERR: {}\n", switch_output.stderr);

            return Err(anyhow::anyhow!(
                "Failed to switchover PostgreSQL leader from {} to {}",
                leader.name,
                target_conf_node.name
            ));
        }
    } else {
        // Trigger failover using patronictl since there is no leader
        println!(
            "\nPromoting node {} to PostgreSQL leader using Patroni...",
            target_conf_node.name
        );

        let target_interactor = get_server_interactor(&target_conf_node.name)?;
        let failover_cmd = format!(
            "sudo patronictl -c /etc/patroni/config.yml failover --candidate {} --force",
            target_conf_node.name
        );
        let failover_output = target_interactor.cmd(&failover_cmd)?;

        if failover_output.exit_code != 0 {
            println!("\nFailed to failover PostgreSQL leader");

            println!("\nSTDOUT: {}", failover_output.stdout);
            println!("\nSTDERR\n: {}", failover_output.stderr);

            return Err(anyhow::anyhow!("Failed to failover PostgreSQL leader"));
        }
    }

    println!(
        "\nPROMOTION TO LEADER COMPLETE FOR NODE '{}'",
        target_conf_node.name
    );

    Ok(())
}
