use std::path::Path;
use std::process::Command;

use crane::postgres_unit::helper::postgres_get_primary;

// RUN:
// cargo nextest run --test deploy -- test_deploy --nocapture

#[tokio::test]
async fn test_deploy() {
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
    let config = crane::config::read_config_toml_file(config_path).expect("Failed to load config");
    crane::commands::deploy::run_deploy_command(&config, config_path, true)
        .await
        .expect("deploy failed");

    // ASSERT this machine can curl at myapp.example.com
    let curl_myapp = Command::new("curl")
        .args([
            "-w",
            "\\n",
            "-L",
            "-k",
            "-i",
            "--resolve",
            "myapp.example.com:80:127.0.0.1",
            "--resolve",
            "myapp.example.com:443:127.0.0.1",
            "http://myapp.example.com",
        ])
        .output()
        .expect("failed to execute curl myapp.example.com");
    let stdout_myapp = String::from_utf8_lossy(&curl_myapp.stdout);
    assert!(curl_myapp.status.success(), "curl myapp.example.com failed");
    assert!(
        stdout_myapp.contains("Hello, myapp!"),
        "expected 'Hello, myapp!' in response, got: {}",
        stdout_myapp
    );

    // ASSERT this machine can curl at myapp2.example.com
    let curl_myapp2 = Command::new("curl")
        .args([
            "-w",
            "\\n",
            "-L",
            "-k",
            "-i",
            "--resolve",
            "myapp2.example.com:80:127.0.0.1",
            "--resolve",
            "myapp2.example.com:443:127.0.0.1",
            "http://myapp2.example.com",
        ])
        .output()
        .expect("failed to execute curl myapp2.example.com");
    let stdout_myapp2 = String::from_utf8_lossy(&curl_myapp2.stdout);
    assert!(
        curl_myapp2.status.success(),
        "curl myapp2.example.com failed"
    );
    assert!(
        stdout_myapp2.contains("Hello, myapp2!"),
        "expected 'Hello, myapp2!' in response, got: {}",
        stdout_myapp2
    );

    // ASSERT we can curl to myapp.example.com/pg and get a 200 status
    // curl -w "\\n%{http_code}\\n" -L -k -s \
    // --resolve myapp2.example.com:80:127.0.0.1 --resolve myapp2.example.com:443:127.0.0.1 \
    // --resolve myapp.example.com:80:127.0.0.1 --resolve myapp.example.com:443:127.0.0.1  \
    // http://myapp.example.com/pg
    let curl_pg = Command::new("curl")
        .args([
            "-w",
            "\\n%{http_code}",
            "-L",
            "-k",
            "-s",
            "--resolve",
            "myapp.example.com:80:127.0.0.1",
            "--resolve",
            "myapp.example.com:443:127.0.0.1",
            "http://myapp.example.com/pg",
        ])
        .output()
        .expect("failed to execute curl myapp.example.com/pg");
    let stdout_pg = String::from_utf8_lossy(&curl_pg.stdout);
    let stderr_pg = String::from_utf8_lossy(&curl_pg.stderr);
    let status_str = stdout_pg.lines().last().unwrap_or("").trim();
    if status_str != "200" {
        panic!(
            "\n================== CURL myapp.example.com/pg FAILED ==================\n\
             HTTP Status: {}\n\
             STDOUT:\n{}\n\
             STDERR:\n{}\n\
             =======================================================================\n",
            status_str, stdout_pg, stderr_pg
        );
    }

    // ASSERT we can curl to myapp2.example.com/pg and get a 200 status
    let curl_pg = Command::new("curl")
        .args([
            "-w",
            "\\n%{http_code}",
            "-L",
            "-k",
            "-s",
            "--resolve",
            "myapp2.example.com:80:127.0.0.1",
            "--resolve",
            "myapp2.example.com:443:127.0.0.1",
            "http://myapp2.example.com/pg",
        ])
        .output()
        .expect("failed to execute curl myapp2.example.com/pg");
    let stdout_pg = String::from_utf8_lossy(&curl_pg.stdout);
    let stderr_pg = String::from_utf8_lossy(&curl_pg.stderr);
    let status_str = stdout_pg.lines().last().unwrap_or("").trim();
    if status_str != "200" {
        panic!(
            "\n================== CURL myapp2.example.com/pg FAILED ==================\n\
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

    let primary_node = postgres_get_primary(&config)
        .expect("Failed to get leader node")
        .expect("No active PostgreSQL leader found");

    let primary_interactor = crane::postgres_unit::helper::connect_to_node(&primary_node, &config)
        .expect("Failed to connect to primary node");

    let pg_hba_path = "/etc/postgresql/17/main/pg_hba.conf";
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

    // ASSERT this machine can psql with user="u1" password="u1" database="mydb"
    let psql_u1 = Command::new("psql")
        .env("PGPASSWORD", "u1")
        .args([
            "-h",
            "127.0.0.1",
            "-U",
            "u1",
            "-d",
            "mydb",
            "-c",
            "SELECT 1",
        ])
        .output()
        .expect("failed to execute psql for user u1");
    let stdout_u1 = String::from_utf8_lossy(&psql_u1.stdout);
    assert!(
        psql_u1.status.success(),
        "psql for user u1 failed: {}",
        String::from_utf8_lossy(&psql_u1.stderr)
    );
    assert!(
        stdout_u1.contains('1'),
        "expected '1' in psql response for user u1"
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
    let env_file_content = primary_interactor
        .cmd("sudo cat /app_config/myapp/.env")
        .expect("failed to read /app_config/myapp/.env")
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
    let haproxy_5000 = primary_interactor
        .cmd("pg_isready -h 127.0.0.1 -p 5000")
        .expect("failed to query pg_isready on port 5000");
    assert_eq!(
        haproxy_5000.exit_code, 0,
        "HAProxy on port 5000 is not accepting connections: {}",
        haproxy_5000.stderr
    );

    let haproxy_5001 = primary_interactor
        .cmd("pg_isready -h 127.0.0.1 -p 5001")
        .expect("failed to query pg_isready on port 5001");
    assert_eq!(
        haproxy_5001.exit_code, 0,
        "HAProxy on port 5001 is not accepting connections: {}",
        haproxy_5001.stderr
    );

    // Verify pre-deploy script executed on the node
    let before_deploy_content = primary_interactor
        .cmd("cat /tmp/before-deploy.txt")
        .expect("failed to read /tmp/before-deploy.txt")
        .stdout;
    assert_eq!(
        before_deploy_content.trim(),
        "1",
        "Expected /tmp/before-deploy.txt content to be '1'"
    );
}
