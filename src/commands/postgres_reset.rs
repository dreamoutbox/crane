use crate::config;
use crate::etcd_unit::etcd_clear_dcs_state;
use crate::helper::config::config_get_nodes;
use crate::postgres_unit::helper::get_pg_version;
use crate::server_interactor::get_server_interactor;

pub fn run_postgres_reset_cmd(config: &config::Config, force: bool) -> anyhow::Result<()> {
    let pg_nodes = config_get_nodes(config, "postgres");
    if pg_nodes.is_empty() {
        println!("No PostgreSQL nodes found in configuration.");
        return Ok(());
    }

    if !force {
        print!("Are you sure you want to reset the PostgreSQL cluster? [y/N]: ");
        use std::io::Write;
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let input = input.trim().to_lowercase();
        if input != "y" && input != "yes" {
            println!("Reset cancelled.");
            return Ok(());
        }
    }

    let pg_version = get_pg_version(config);

    // 1. Stop Patroni on all nodes
    println!("\nStopping Patroni on all PostgreSQL nodes...");
    for node in &pg_nodes {
        println!("Connecting to node {}...", node.name);
        let interactor = get_server_interactor(&node.name)?;

        println!("\tStopping Patroni service...");
        let _ = interactor.stop_service("patroni");

        // Also kill any remaining postgres processes
        let _ = interactor.kill_postgres_processes();
    }

    // 2. Clear DCS (etcd) keys for the cluster to prevent conflicts
    if let Some(first_node) = pg_nodes.first() {
        // run on first node
        let interactor = get_server_interactor(&first_node.name)?;

        println!("Clearing Etcd DCS cluster state...");
        etcd_clear_dcs_state(&*interactor);
    }

    // 3. Remove PostgreSQL data directory on all nodes
    println!("\nRemoving PostgreSQL data directory on all nodes...");
    for node in &pg_nodes {
        println!("Connecting to node {}...", node.name);
        let interactor = get_server_interactor(&node.name)?;
        let data_dir = format!("/var/lib/postgresql/{}/main", pg_version);
        println!("\tRemoving directory {}...", data_dir);
        interactor.rm(&data_dir)?;
    }

    // 4. Start Patroni on all nodes
    println!("\nStarting Patroni on all PostgreSQL nodes...");
    for node in &pg_nodes {
        println!("Connecting to node {}...", node.name);
        let interactor = get_server_interactor(&node.name)?;

        println!("\tStarting Patroni service...");
        interactor.start_service("patroni")?;
    }

    println!("\nPOSTGRESQL RESET COMPLETE");

    Ok(())
}
