use crate::postgres_unit::helper::connect_to_node;
use crate::postgres_unit::helper::find_node_config_with_fallback;
use crate::postgres_unit::helper::postgres_get_primary;

pub fn run_promote_cmd(
    config: &crate::config::Config,
    target_node_str: &str,
) -> anyhow::Result<()> {
    let target_conf = find_node_config_with_fallback(target_node_str, &config)
        .ok_or_else(|| anyhow::anyhow!("Node '{}' not found in configuration", target_node_str))?;

    if !target_conf.roles.contains(&"postgres".to_string()) {
        anyhow::bail!(
            "Node '{}' does not have the 'postgres' role",
            target_node_str
        );
    }

    let current_leader = postgres_get_primary(&config)?;

    if let Some(ref leader) = current_leader {
        if leader.internal_ip == target_conf.internal_ip {
            println!(
                "Node '{}' is already the active PostgreSQL leader.",
                target_node_str
            );
            return Ok(());
        }

        // Trigger switchover using patronictl
        println!(
            "\nSwitching over PostgreSQL leader from {} to {} using Patroni...",
            leader.name, target_conf.name
        );
        let target_interactor = connect_to_node(&target_conf, &config)?;
        let switch_cmd = format!(
            "sudo patronictl -c /etc/patroni/config.yml switchover --leader {} --candidate {} --scheduled now --force",
            leader.name, target_conf.name
        );
        let switch_output = target_interactor.cmd(&switch_cmd)?;

        if switch_output.exit_code != 0 {
            println!(
                "\nFailed to switchover PostgreSQL leader from {} to {}",
                leader.name, target_conf.name
            );

            println!("\nSTDOUT: {}", switch_output.stdout);
            println!("\nSTDERR: {}\n", switch_output.stderr);

            return Err(anyhow::anyhow!(
                "Failed to switchover PostgreSQL leader from {} to {}",
                leader.name,
                target_conf.name
            ));
        }
    } else {
        // Trigger failover using patronictl since there is no leader
        println!(
            "\nPromoting node {} to PostgreSQL leader using Patroni...",
            target_conf.name
        );

        let target_interactor = connect_to_node(&target_conf, &config)?;
        let failover_cmd = format!(
            "sudo patronictl -c /etc/patroni/config.yml failover --candidate {} --force",
            target_conf.name
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
        target_conf.name
    );

    Ok(())
}
