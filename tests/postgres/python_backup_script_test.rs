// RUN:
// RUST_BACKTRACE=1 cargo nextest run test_python_backup_script -- --no-capture

#[tokio::test]
async fn test_python_backup_script() {
    let config_path = std::path::Path::new("tests/postgres/crane.toml");
    let config = read_config_toml_file(config_path).expect("Failed to load crane.toml");

    // Deploy the cluster
    crane::commands::deploy::run_deploy_command(&config, config_path, true)
        .await
        .expect("deploy failed");

    // Retrieve pg nodes configuration
    let pg_nodes = crane::helper::config::config_get_nodes(&config, "postgres");
    assert!(
        pg_nodes.len() >= 3,
        "Expected at least 3 nodes (vps1, vps2, vps3) for python backup script test"
    );

    // Discover the current primary node dynamically
    let primary_node = postgres_get_primary(&config)
        .expect("Failed to get primary node")
        .expect("No active PostgreSQL leader found");

    // Identify the replicas dynamically
    let replicas: Vec<&crane::config::NodeConfig> = pg_nodes
        .iter()
        .filter(|n| n.name != primary_node.name)
        .collect();
    assert_eq!(replicas.len(), 2, "Expected exactly 2 replicas");
    let replica1_node = replicas[0];
    let replica2_node = replicas[1];

    // Connect to all nodes via ServerInteractor explicitly
    let primary_interactor = crane::postgres_unit::helper::connect_to_node(&primary_node, &config)
        .expect("primary connection failed");
    let replica1_interactor = crane::postgres_unit::helper::connect_to_node(replica1_node, &config)
        .expect("replica1 connection failed");
    let replica2_interactor = crane::postgres_unit::helper::connect_to_node(replica2_node, &config)
        .expect("replica2 connection failed");

    // Wait for all replica nodes to be fully ready
    println!("Waiting for replica nodes to be fully ready...");
    let ready = crane::postgres_unit::helper::pg_cluster_wait_all_nodes_ready(
        &*primary_interactor,
        &pg_nodes,
    );
    assert!(ready, "Not all PostgreSQL nodes became ready in time");

    // Step 1: Run the python backup script on the primary node. It should succeed.
    println!(
        "Running python backup script on primary node: {}",
        primary_node.name
    );
    let out = primary_interactor
        .cmd("sudo python3 /opt/crane/postgres-backup.py full")
        .expect("Failed to run backup script on primary");
    assert_eq!(
        out.exit_code, 0,
        "Backup on primary node {} should succeed.\nStdout: {}\nStderr: {}",
        primary_node.name, out.stdout, out.stderr
    );
    assert!(
        out.stdout.contains("Backup completed successfully."),
        "Expected backup success message on primary node"
    );

    // Step 2: Run the python backup script on the replica nodes. They should fail.
    println!(
        "Running python backup script on replica node: {}",
        replica1_node.name
    );
    let out = replica1_interactor
        .cmd("sudo python3 /opt/crane/postgres-backup.py full")
        .expect("Failed to run backup script on replica1");
    assert_eq!(
        out.exit_code, 1,
        "Backup on replica node {} should fail.\nStdout: {}\nStderr: {}",
        replica1_node.name, out.stdout, out.stderr
    );
    assert!(
        out.stdout
            .contains("Backups can only be run on the primary node")
            || out
                .stderr
                .contains("Backups can only be run on the primary node"),
        "Expected failure message about not being the primary node on {}.\nStdout: {}\nStderr: {}",
        replica1_node.name,
        out.stdout,
        out.stderr
    );

    println!(
        "Running python backup script on replica node: {}",
        replica2_node.name
    );
    let out = replica2_interactor
        .cmd("sudo python3 /opt/crane/postgres-backup.py full")
        .expect("Failed to run backup script on replica2");
    assert_eq!(
        out.exit_code, 1,
        "Backup on replica node {} should fail.\nStdout: {}\nStderr: {}",
        replica2_node.name, out.stdout, out.stderr
    );
    assert!(
        out.stdout
            .contains("Backups can only be run on the primary node")
            || out
                .stderr
                .contains("Backups can only be run on the primary node"),
        "Expected failure message about not being the primary node on {}.\nStdout: {}\nStderr: {}",
        replica2_node.name,
        out.stdout,
        out.stderr
    );

    // Step 3: Promote replica2_node follower node to leader
    println!("Promoting follower node: {}", replica2_node.name);
    crane::commands::postgres_promote::run_promote_cmd(&config, &replica2_node.name)
        .expect("Failed to promote follower node");

    // Poll for status update to reflect promotion
    let mut promoted_node_is_leader = false;
    for _ in 0..15 {
        if let Ok(st) = crane::commands::postgres_status::get_postgres_status_wrapper(&config).await
        {
            if let Some(node) = st
                .postgres
                .iter()
                .find(|n| n.hostname == replica2_node.name)
            {
                if node.role == "Leader" && st.haproxy.primary == replica2_node.name {
                    promoted_node_is_leader = true;
                    break;
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    assert!(
        promoted_node_is_leader,
        "Expected promoted node '{}' to become leader, but it did not",
        replica2_node.name
    );

    // Step 4: Run the python backup script on the newly promoted leader node. It should succeed.
    println!(
        "Running python backup script on newly promoted leader node: {}",
        replica2_node.name
    );
    let out = replica2_interactor
        .cmd("sudo python3 /opt/crane/postgres-backup.py full")
        .expect("Failed to run backup script on promoted leader");
    assert_eq!(
        out.exit_code, 0,
        "Backup on promoted leader node {} should succeed.\nStdout: {}\nStderr: {}",
        replica2_node.name, out.stdout, out.stderr
    );
    assert!(
        out.stdout.contains("Backup completed successfully."),
        "Expected backup success message on promoted leader node"
    );

    // Wait for the remaining nodes (now replicas) to be fully ready after promotion
    println!("Waiting for remaining replica nodes to be fully ready after promotion...");
    let ready = crane::postgres_unit::helper::pg_cluster_wait_all_nodes_ready(&*replica2_interactor, &pg_nodes);
    assert!(ready, "Not all PostgreSQL nodes became ready in time after promotion");

    // Step 5: Run the python backup script on the remaining nodes (now replicas). They should fail.
    println!(
        "Running python backup script on now-replica node: {}",
        replica1_node.name
    );
    let out = replica1_interactor
        .cmd("sudo python3 /opt/crane/postgres-backup.py full")
        .expect("Failed to run backup script");
    assert_eq!(
        out.exit_code, 1,
        "Backup on now-replica node {} should fail.\nStdout: {}\nStderr: {}",
        replica1_node.name, out.stdout, out.stderr
    );
    assert!(
        out.stdout
            .contains("Backups can only be run on the primary node")
            || out
                .stderr
                .contains("Backups can only be run on the primary node"),
        "Expected failure message about not being the primary node on {}.\nStdout: {}\nStderr: {}",
        replica1_node.name,
        out.stdout,
        out.stderr
    );

    println!(
        "Running python backup script on now-replica node: {}",
        primary_node.name
    );
    let out = primary_interactor
        .cmd("sudo python3 /opt/crane/postgres-backup.py full")
        .expect("Failed to run backup script");
    assert_eq!(
        out.exit_code, 1,
        "Backup on now-replica node {} should fail.\nStdout: {}\nStderr: {}",
        primary_node.name, out.stdout, out.stderr
    );
    assert!(
        out.stdout
            .contains("Backups can only be run on the primary node")
            || out
                .stderr
                .contains("Backups can only be run on the primary node"),
        "Expected failure message about not being the primary node on {}.\nStdout: {}\nStderr: {}",
        primary_node.name,
        out.stdout,
        out.stderr
    );
}
