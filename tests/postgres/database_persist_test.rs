// RUN:
// RUST_BACKTRACE=1 cargo nextest run test_database_persist_after_deploy -- --no-capture

#[tokio::test]
async fn test_database_persist_after_deploy() {
    let config_path = std::path::Path::new("tests/postgres/crane.with_app.toml");
    let config = read_config_toml_file(config_path).expect("Failed to load crane.toml");

    // Deploy
    crane::commands::deploy::run_deploy_command(&config, config_path, true)
        .await
        .expect("deploy failed");

    // Retrieve leader node and connect
    let primary_node = postgres_get_primary(&config)
        .expect("Failed to get leader node")
        .expect("No active PostgreSQL leader found");

    let interactor = crane::postgres_unit::helper::connect_to_node(&primary_node, &config)
        .expect("Failed to connect to primary node");

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
    let interactor = crane::postgres_unit::helper::connect_to_node(&primary_node, &config)
        .expect("Failed to connect to primary node");

    // assert value is still 1
    let val_str_after = run_sql(&*interactor, "SELECT value FROM api_counter WHERE id = 1;");
    assert_eq!(val_str_after, "1");
}
