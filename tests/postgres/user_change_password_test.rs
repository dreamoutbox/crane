// RUN:
// RUST_LIB_BACKTRACE=0 RUST_BACKTRACE=1 cargo nextest run test_user_change_password -- --no-capture

use std::process::Command;

#[test]
fn test_user_change_password() {
    //Deploy config with postgres user old password
    let old_config_path =
        std::path::Path::new("tests/postgres/crane.postgres_user_old_password.toml");
    crane::commands::deploy::run(old_config_path, false).expect("deploy failed");

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
    crane::commands::deploy::run(new_config_path, false).expect("deploy failed");

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

fn try_connect(user: &str, password: &str, db: &str) -> Result<String, String> {
    let output = Command::new("psql")
        .env("PGPASSWORD", password)
        .args(["-h", "127.0.0.1", "-U", user, "-d", db, "-c", "SELECT 1"])
        .output();

    match output {
        Ok(out) => {
            if out.status.success() {
                Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
            } else {
                Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

fn allow_host_connection(config_path: &std::path::Path) {
    use crane::postgres_unit::helper::postgres_get_leader;

    let config = crane::config::load_config(config_path).expect("Failed to load config");
    let primary_node = postgres_get_leader(&config)
        .expect("Failed to get leader node")
        .expect("No active PostgreSQL leader found");
    let primary_interactor = crane::postgres_unit::helper::connect_to_node(&primary_node, &config)
        .expect("Failed to connect to primary node");

    let pg_version = crane::postgres_unit::helper::get_pg_version(&config);
    let pg_hba_path = format!("/etc/postgresql/{}/main/pg_hba.conf", pg_version);
    let add_rule_cmd = format!(
        "echo 'host all all 10.0.0.0/24 scram-sha-256' | sudo tee -a {}",
        pg_hba_path
    );
    primary_interactor
        .cmd(&add_rule_cmd)
        .expect("failed to add pg_hba rule");
    primary_interactor
        .cmd("sudo -u postgres psql -c 'SELECT pg_reload_conf();'")
        .expect("failed to reload pg config");
}
