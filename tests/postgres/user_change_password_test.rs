use crate::helper::{pg_allow_host_machine, reset_docker_compose, try_connect};

// RUN:
// RUST_LIB_BACKTRACE=0 RUST_BACKTRACE=1 cargo nextest run test_user_change_password -- --no-capture

#[tokio::test]
async fn test_user_change_password() {
    println!("Recreating Docker compose...");
    reset_docker_compose().await;

    //Deploy config with postgres user old password
    let old_config_path =
        std::path::Path::new("tests/postgres/crane.postgres_user_old_password.toml");
    let old_config =
        crane::config::read_config_toml_file(old_config_path).expect("Failed to load config");
    crane::commands::deploy::run_deploy_command(&old_config, old_config_path, true)
        .await
        .expect("deploy failed");

    // Allow host machine connection to Docker container
    pg_allow_host_machine(&old_config);

    //Assert user can connect postgres with old-password
    let conn_old = try_connect("u1", "old-password", "mydb");
    assert!(
        conn_old.is_ok(),
        "Expected connection with old password to succeed, got error: {:?}",
        conn_old.err()
    );

    //Deploy config with postgres user new password
    let new_config_path =
        std::path::Path::new("tests/postgres/crane.postgres_user_new_password.toml");
    let new_config =
        crane::config::read_config_toml_file(new_config_path).expect("Failed to load config");
    crane::commands::deploy::run_deploy_command(&new_config, new_config_path, true)
        .await
        .expect("deploy failed");

    // Allow host machine connection again since configuration was redeployed
    pg_allow_host_machine(&new_config);

    //Assert user can't connect postgres with old password
    let conn_old_fail = try_connect("u1", "old-password", "mydb");
    assert!(
        conn_old_fail.is_err(),
        "Expected connection with old password to fail, but it succeeded"
    );
    let err_msg = conn_old_fail.unwrap_err();
    // dbg!(&err_msg);
    //Expect error:
    //"psql: error: connection to server at \"127.0.0.1\", port 5432 failed:
    //FATAL:  password authentication failed for user \"u1\"\nconnection to server at
    //\"127.0.0.1\", port 5432 failed: FATAL:  password authentication failed for user \"u1\""
    assert!(
        err_msg.contains("password authentication failed"),
        "Expected password authentication failed, got: {}",
        err_msg
    );

    //Assert user CAN connect postgres with new-password
    let conn_new = try_connect("u1", "new-password", "mydb");
    assert!(
        conn_new.is_ok(),
        "Expected connection with new password to succeed, got error: {:?}",
        conn_new.err()
    );
}
