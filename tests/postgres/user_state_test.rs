// RUN:
// RUST_LIB_BACKTRACE=0 RUST_BACKTRACE=1 cargo nextest run test_user_state -- --no-capture

use crate::common_helper::reset_docker_compose;
use crate::helper::{pg_allow_host_machine, try_connect};

#[tokio::test]
async fn test_user_state() {
    println!("Recreating Docker compose...");
    reset_docker_compose().await;

    //Deploy config with postgres user present
    let user_present_config_path =
        std::path::Path::new("tests/postgres/crane.postgres_user_present.toml");
    let user_present_config = crane::config::read_config_toml_file(user_present_config_path)
        .expect("Failed to load config");
    crane::commands::deploy::run_deploy_command(
        &user_present_config,
        user_present_config_path,
        true,
    )
    .await
    .expect("deploy failed");

    // Allow host machine connection to Docker container
    pg_allow_host_machine(&user_present_config);

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
    crane::commands::deploy::run_deploy_command(&user_absent_config, user_absent_config_path, true)
        .await
        .expect("deploy failed");

    // Allow host machine connection again since configuration was redeployed
    pg_allow_host_machine(&user_absent_config);

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
