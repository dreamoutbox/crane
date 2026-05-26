use std::path::Path;
use std::process::Command;

// RUN:
// cargo nextest run --test deploy -- test_deploy --nocapture

#[test]
fn test_deploy() {
    // 1. Build Go demo app
    let go_build = Command::new("go")
        .arg("build")
        .current_dir("demo")
        .output()
        .expect("failed to run go build");
    assert!(
        go_build.status.success(),
        "go build failed: {}",
        String::from_utf8_lossy(&go_build.stderr)
    );

    // 2. Deploy app configuration to VPS nodes
    let config_path = Path::new("demo/crane.toml");
    crane::commands::deploy::run(config_path, crane::server_interactor::get_interactor)
        .expect("deploy failed");

    // ASSERT this machine can curl at myapp.localhost
    let curl_myapp = Command::new("curl")
        .args([
            "-w",
            "\\n",
            "-L",
            "-k",
            "-i",
            "--resolve",
            "myapp.localhost:80:127.0.0.1",
            "http://myapp.localhost",
        ])
        .output()
        .expect("failed to execute curl myapp.localhost");
    let stdout_myapp = String::from_utf8_lossy(&curl_myapp.stdout);
    assert!(curl_myapp.status.success(), "curl myapp.localhost failed");
    assert!(
        stdout_myapp.contains("Hello, myapp!"),
        "expected 'Hello, myapp!' in response, got: {}",
        stdout_myapp
    );

    // ASSERT this machine can curl at myapp2.localhost
    let curl_myapp2 = Command::new("curl")
        .args([
            "-w",
            "\\n",
            "-L",
            "-k",
            "-i",
            "--resolve",
            "myapp2.localhost:80:127.0.0.1",
            "http://myapp2.localhost",
        ])
        .output()
        .expect("failed to execute curl myapp2.localhost");
    let stdout_myapp2 = String::from_utf8_lossy(&curl_myapp2.stdout);
    assert!(curl_myapp2.status.success(), "curl myapp2.localhost failed");
    assert!(
        stdout_myapp2.contains("Hello, myapp2!"),
        "expected 'Hello, myapp2!' in response, got: {}",
        stdout_myapp2
    );

    // ASSERT we can curl to myapp.localhost/pg and get a 200 status
    let curl_pg = Command::new("curl")
        .args([
            "-w",
            "\\n%{http_code}",
            "-L",
            "-k",
            "-s",
            "--resolve",
            "myapp.localhost:80:127.0.0.1",
            "http://myapp.localhost/pg",
        ])
        .output()
        .expect("failed to execute curl myapp.localhost/pg");
    let stdout_pg = String::from_utf8_lossy(&curl_pg.stdout);
    let stderr_pg = String::from_utf8_lossy(&curl_pg.stderr);
    let status_str = stdout_pg.lines().last().unwrap_or("").trim();
    if status_str != "200" {
        panic!(
            "\n================== CURL myapp.localhost/pg FAILED ==================\n\
             HTTP Status: {}\n\
             STDOUT:\n{}\n\
             STDERR:\n{}\n\
             =======================================================================\n",
            status_str, stdout_pg, stderr_pg
        );
    }

    // ASSERT we can curl to myapp2.localhost/pg and get a 200 status
    let curl_pg = Command::new("curl")
        .args([
            "-w",
            "\\n%{http_code}",
            "-L",
            "-k",
            "-s",
            "--resolve",
            "myapp2.localhost:80:127.0.0.1",
            "http://myapp2.localhost/pg",
        ])
        .output()
        .expect("failed to execute curl myapp2.localhost/pg");
    let stdout_pg = String::from_utf8_lossy(&curl_pg.stdout);
    let stderr_pg = String::from_utf8_lossy(&curl_pg.stderr);
    let status_str = stdout_pg.lines().last().unwrap_or("").trim();
    if status_str != "200" {
        panic!(
            "\n================== CURL myapp2.localhost/pg FAILED ==================\n\
             HTTP Status: {}\n\
             STDOUT:\n{}\n\
             STDERR:\n{}\n\
             ========================================================================\n",
            status_str, stdout_pg, stderr_pg
        );
    }

    // To allow connection from the host machine
    //(IP 10.0.0.1 on docker bridge network 10.0.0.0/24),
    // we need to temporarily add a pg_hba.conf entry
    // to the deployed primary postgres database and reload.
    let config = crane::config::load_config(config_path).expect("Failed to load crane.toml");

    let primary_node = crane::postgres_unit::tasks::postgres_get_leader(
        &config,
        crane::server_interactor::get_interactor,
    )
    .expect("Failed to get leader node")
    .expect("No active PostgreSQL leader found");

    let interactor = crane::postgres_unit::helper::connect_to_node(
        &primary_node,
        &config,
        crane::server_interactor::get_interactor,
    )
    .expect("Failed to connect to primary node");

    let pg_hba_path = "/etc/postgresql/17/main/pg_hba.conf";
    let add_rule_cmd = format!(
        "echo 'host all all 10.0.0.0/24 scram-sha-256' | sudo tee -a {}",
        pg_hba_path
    );
    interactor
        .cmd(&add_rule_cmd)
        .expect("failed to add pg_hba rule");
    interactor
        .cmd("sudo -u postgres psql -c 'SELECT pg_reload_conf();'")
        .expect("failed to reload pg config");

    // ASSERT this machine can psql with user="app1" password="app1" database="mydb"
    let psql_app1 = Command::new("psql")
        .env("PGPASSWORD", "app1")
        .args([
            "-h",
            "127.0.0.1",
            "-U",
            "app1",
            "-d",
            "mydb",
            "-c",
            "SELECT 1",
        ])
        .output()
        .expect("failed to execute psql for user app1");
    let stdout_app1 = String::from_utf8_lossy(&psql_app1.stdout);
    assert!(
        psql_app1.status.success(),
        "psql for user app1 failed: {}",
        String::from_utf8_lossy(&psql_app1.stderr)
    );
    assert!(
        stdout_app1.contains('1'),
        "expected '1' in psql response for user app1"
    );

    // ASSERT this machine can psql with user="u2" password="u2" database="mydb"
    let psql_u2 = Command::new("psql")
        .env("PGPASSWORD", "u2")
        .args([
            "-h",
            "127.0.0.1",
            "-U",
            "u2",
            "-d",
            "mydb",
            "-c",
            "SELECT 1",
        ])
        .output()
        .expect("failed to execute psql for user u2");
    let stdout_u2 = String::from_utf8_lossy(&psql_u2.stdout);
    assert!(
        psql_u2.status.success(),
        "psql for user u2 failed: {}",
        String::from_utf8_lossy(&psql_u2.stderr)
    );
    assert!(
        stdout_u2.contains('1'),
        "expected '1' in psql response for user u2"
    );

    // Verify injected env variables in app .env
    let env_file_content = interactor
        .cmd("sudo cat /etc/crane/myapp/.env")
        .expect("failed to read /etc/crane/myapp/.env")
        .stdout;

    assert!(
        env_file_content.contains("POSTGRES_MYDB_LEADER=postgresql://u1:u1@127.0.0.1:5000/mydb"),
        "Expected POSTGRES_MYDB_LEADER in env file, got: {}",
        env_file_content
    );
    assert!(
        env_file_content.contains("POSTGRES_MYDB_URI=postgresql://u1:u1@127.0.0.1:5000/mydb"),
        "Expected POSTGRES_MYDB_URI in env file, got: {}",
        env_file_content
    );
    assert!(
        env_file_content.contains("POSTGRES_MYDB_FOLLOWER=postgresql://u1:u1@127.0.0.1:5001/mydb"),
        "Expected POSTGRES_MYDB_FOLLOWER in env file, got: {}",
        env_file_content
    );

    // Verify HAProxy is listening and healthy on ports 5000 and 5001
    let haproxy_5000 = interactor
        .cmd("pg_isready -h 127.0.0.1 -p 5000")
        .expect("failed to query pg_isready on port 5000");
    assert_eq!(
        haproxy_5000.exit_code, 0,
        "HAProxy on port 5000 is not accepting connections: {}",
        haproxy_5000.stderr
    );

    let haproxy_5001 = interactor
        .cmd("pg_isready -h 127.0.0.1 -p 5001")
        .expect("failed to query pg_isready on port 5001");
    assert_eq!(
        haproxy_5001.exit_code, 0,
        "HAProxy on port 5001 is not accepting connections: {}",
        haproxy_5001.stderr
    );
}
