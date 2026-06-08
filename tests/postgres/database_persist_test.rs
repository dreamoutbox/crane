// RUN:
// RUST_BACKTRACE=1 cargo nextest run test_database_persist_after_deploy -- --no-capture

use crate::common_helper::reset_docker_compose;
use crate::helper::run_sql;
use crane::{
    config::read_config_toml_file, postgres_unit::helper::pg_get_primary,
    server_interactor::get_server_interactor,
};

#[tokio::test]
async fn test_database_persist_after_deploy() {
    println!("Recreating Docker compose...");
    reset_docker_compose().await;

    let config_path = std::path::Path::new("demo/crane.minimal.toml");
    let config = read_config_toml_file(config_path).expect("Failed to load crane.toml");

    // Deploy
    crane::commands::deploy::run_deploy_command(&config, config_path, true)
        .await
        .expect("deploy failed");

    // Retrieve leader node and connect
    let primary_node = pg_get_primary(&config)
        .expect("Failed to get leader node")
        .expect("No active PostgreSQL leader found");

    let interactor =
        get_server_interactor(&primary_node.name).expect("Failed to connect to primary node");

    // create table api_counter in database
    run_sql(
        &*interactor,
        "DROP TABLE IF EXISTS api_counter; CREATE TABLE api_counter (id INT PRIMARY KEY, value INT);",
    );

    // INSERT INTO api_counter (id, value) VALUES (1, 1) ON CONFLICT (id) DO NOTHING
    run_sql(
        &*interactor,
        "INSERT INTO api_counter (id, value) VALUES (1, 1) ON CONFLICT (id) DO NOTHING;",
    );

    // verify value is inserted
    let val_str = run_sql(&*interactor, "SELECT value FROM api_counter WHERE id = 1;");
    assert_eq!(val_str, "1");

    // deploy again
    crane::commands::deploy::run_deploy_command(&config, config_path, true)
        .await
        .expect("deploy failed");

    // Reconnect to leader after deploy
    let interactor =
        get_server_interactor(&primary_node.name).expect("Failed to connect to primary node");

    // assert value is still 1
    let val_str_after = run_sql(&*interactor, "SELECT value FROM api_counter WHERE id = 1;");
    assert_eq!(val_str_after, "1");
}
