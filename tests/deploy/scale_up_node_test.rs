use std::path::Path;

use crate::common_helper::reset_docker_compose;

// RUN:
// RUST_BACKTRACE=1 cargo nextest run test_scale_up_node --nocapture

#[tokio::test]
async fn test_scale_up_node() {
    println!("Recreating Docker compose...");
    reset_docker_compose().await;

    println!("STEP 1: deploy with only 1 node");
    // 2. Deploy app configuration to VPS nodes
    let config_1node_path = Path::new("tests/deploy/crane.1node.toml");
    let config_1node =
        crane::config::read_config_toml_file(config_1node_path).expect("Failed to load config");
    crane::commands::deploy::run_deploy_command(&config_1node, config_1node_path, true)
        .await
        .expect("deploy 1 node failed");

    println!("STEP 2: scale up to 3 nodes");
    let config_3node_path = Path::new("tests/deploy/crane.3node.toml");
    let config_3node =
        crane::config::read_config_toml_file(config_3node_path).expect("Failed to load config");
    crane::commands::deploy::run_deploy_command(&config_3node, config_3node_path, true)
        .await
        .expect("deploy 3 node failed");
}
