// RUN:
// RUST_BACKTRACE=1 cargo nextest run --test postgres -- test_promote --no-capture

#[tokio::test]
async fn test_promote() {
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

    // get a follower node
    let follower = status
        .postgres
        .iter()
        .find(|node| node.role == "Follower")
        .expect("No follower node found in the cluster status");

    // run promote function on a follower node
    crane::commands::postgres_promote::run_promote_cmd(&config, &follower.hostname)
        .expect("Failed to promote follower node");

    // poll for status update to reflect promotion
    let mut promoted_node_is_leader = false;
    let mut new_status = None;
    for _ in 0..15 {
        if let Ok(st) = crane::commands::postgres_status::get_postgres_status_wrapper(&config).await
        {
            if let Some(node) = st.postgres.iter().find(|n| n.hostname == follower.hostname) {
                dbg!(&node);

                if node.role == "Leader" && st.haproxy.primary == follower.hostname {
                    promoted_node_is_leader = true;
                    new_status = Some(st);
                    break;
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    // assert that the promoted node is leader
    assert!(
        promoted_node_is_leader,
        "Expected promoted node '{}' to become leader, but it did not",
        follower.hostname
    );

    // assert haproxy point to new leader
    let ns = new_status.unwrap();
    assert_eq!(
        ns.haproxy.primary, follower.hostname,
        "HAProxy primary should point to the newly promoted leader node"
    );
}
