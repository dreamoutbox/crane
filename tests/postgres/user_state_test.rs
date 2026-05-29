// RUN:
// RUST_LIB_BACKTRACE=0 RUST_BACKTRACE=1 cargo nextest run test_user_state -- --no-capture

#[test]
fn test_user_state() {
    //Deploy config with postgres user present
    let user_present_config_path =
        std::path::Path::new("tests/postgres/crane.postgres_user_present.toml");
    let user_present_config = crane::config::read_config_toml_file(user_present_config_path)
        .expect("Failed to load config");
    crane::commands::deploy::run(&user_present_config, user_present_config_path, true)
        .expect("deploy failed");

    // Allow host machine connection to Docker container
    allow_host_connection(user_present_config_path);

    //Assert user can connect postgres
    let conn_old = try_connect("u1", "u1", "mydb");
    assert!(
        conn_old.is_ok(),
        "Expected postgres connection to succeed, got error: {:?}",
        conn_old.err()
    );

    //Deploy config with postgres user absent
    let user_absent_config_path =
        std::path::Path::new("tests/postgres/crane.postgres_user_absent.toml");
    let user_absent_config = crane::config::read_config_toml_file(user_absent_config_path)
        .expect("Failed to load config");
    crane::commands::deploy::run(&user_absent_config, user_absent_config_path, true)
        .expect("deploy failed");

    // Allow host machine connection again since configuration was redeployed
    allow_host_connection(user_absent_config_path);

    //Assert user can't connect postgres
    let conn_old_fail = try_connect("u1", "u1", "mydb");
    assert!(
        conn_old_fail.is_err(),
        "Expected connection to fail, but it succeeded"
    );
    let err_msg = conn_old_fail.unwrap_err();
    assert!(
        err_msg.contains("password authentication failed"),
        "Expected password authentication failed, got: {}",
        err_msg
    );
}
