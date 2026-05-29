// RUN:
// RUST_LIB_BACKTRACE=0 RUST_BACKTRACE=1 cargo nextest run test_user_change_password -- --no-capture

use std::process::Command;

#[test]
fn test_user_change_password() {
    //Deploy config with postgres user old password
    let old_config_path =
        std::path::Path::new("tests/postgres/crane.postgres_user_old_password.toml");
    let old_config =
        crane::config::read_config_toml_file(old_config_path).expect("Failed to load config");
    crane::commands::deploy::run(old_config, old_config_path, true).expect("deploy failed");

    // Allow host machine connection to Docker container
    allow_host_connection(old_config_path);

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
    crane::commands::deploy::run(new_config, new_config_path, true).expect("deploy failed");

    // Allow host machine connection again since configuration was redeployed
    allow_host_connection(new_config_path);

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
