use std::path::Path;
use std::process::Command;

use crate::reset_rocky_docker_compose;

// RUN:
// cargo nextest run test_single_deploy_rocky --nocapture

#[tokio::test]
async fn test_single_deploy_rocky() {
    // reset docker compose
    reset_rocky_docker_compose().await;

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

    // 2. Deploy app configuration to A VPS node
    let config_path = Path::new("demo/crane.minimal.rocky.toml");
    let config = crane::config::read_config_toml_file(config_path).expect("Failed to load config");
    crane::commands::deploy::run_deploy_command(&config, config_path, true)
        .await
        .expect("deploy failed");

    // ASSERT we can curl to myapp.example.com/pg and get a 200 status
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
}
