// RUN:
// RUST_BACKTRACE=1 cargo nextest run --test postgres -- test_failover --no-capture

#[tokio::test]
async fn test_failover() {
    let config_path = std::path::Path::new("tests/postgres/crane.toml");
    let config =
        crane::config::read_config_toml_file(config_path).expect("Failed to load crane.toml");

    //Deploy
    crane::commands::deploy::run_deploy_command(&config, config_path, true)
        .await
        .expect("deploy failed");

    // get existing postgres cluster status
    let status = crane::commands::postgres_status::get_postgres_status_wrapper(&config)
        .await
        .expect("Failed to get postgres status");

    // get primary node
    let primary = status
        .postgres
        .iter()
        .find(|node| node.role == "Leader")
        .expect("No leader node found in the cluster status");

    // find primary node config in configuration
    let primary_node_config = config
        .nodes
        .iter()
        .find(|n| n.name == primary.node.name)
        .expect("Failed to find primary node config in nodes list");

    // stop service of primary node
    let primary_interactor = get_server_interactor(&primary_node_config.name)
        .expect("Failed to connect to primary node");
    primary_interactor
        .stop_service("patroni")
        .expect("Failed to stop patroni service on primary node");

    // wait for patroni leader election
    let mut new_leader_elected = false;
    let mut new_status = None;
    for _ in 0..20 {
        if let Ok(st) = crane::commands::postgres_status::get_postgres_status_wrapper(&config).await
        {
            let active_leader = st.postgres.iter().find(|n| n.role == "Leader");
            if let Some(leader) = active_leader {
                if leader.node.name != primary.node.name {
                    new_leader_elected = true;
                    new_status = Some(st);
                    break;
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    // assert new leader
    assert!(
        new_leader_elected,
        "Expected a new leader to be elected after old leader '{}' stopped, but it was not",
        primary.node.name
    );

    // assert haproxy point to new leader
    let ns = new_status.unwrap();
    let new_leader = ns.postgres.iter().find(|n| n.role == "Leader").unwrap();
    assert_eq!(
        ns.haproxy.primary, new_leader.node.name,
        "HAProxy primary should point to the newly elected leader node"
    );
}
