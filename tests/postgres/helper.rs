use crane::postgres_unit::helper::postgres_get_primary;
use crane::server_interactor::get_server_interactor;
use crane::server_interactor::server_interactor_trait::ServerInteractor;
use std::process::Command;

pub fn try_connect(user: &str, password: &str, db: &str) -> Result<String, String> {
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

pub fn pg_allow_host_machine(config: &crane::config::Config) {
    // use crane::postgres_unit::helper::postgres_get_primary;

    // let config = crane::config::read_config_toml_file(config_path).expect("Failed to load config");
    // let primary_node = postgres_get_primary(&config)
    //     .expect("Failed to get leader node")
    //     .expect("No active PostgreSQL leader found");
    // let primary_interactor =
    //     get_server_interactor(&primary_node.name).expect("Failed to connect to primary node");

    // let pg_version = crane::postgres_unit::helper::get_pg_version(&config);
    // let pg_hba_path = format!("/etc/postgresql/{}/main/pg_hba.conf", pg_version);
    // let add_rule_cmd = format!(
    //     "echo 'host all all 10.0.0.0/24 scram-sha-256' | sudo tee -a {}",
    //     pg_hba_path
    // );
    // primary_interactor
    //     .cmd(&add_rule_cmd)
    //     .expect("failed to add pg_hba rule");
    // primary_interactor
    //     .cmd("sudo -u postgres psql -c 'SELECT pg_reload_conf();'")
    //     .expect("failed to reload pg config");

    let primary_node = postgres_get_primary(config)
        .expect("Failed to get leader node")
        .expect("No active PostgreSQL leader found");

    let primary_interactor =
        get_server_interactor(&primary_node.name).expect("Failed to connect to primary node");

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

    primary_interactor
        .firewall_allow_source("10.0.0.0/24")
        .expect("failed to allow subnet in firewall");
    primary_interactor
        .firewall_reload()
        .expect("failed to reload firewall");
}

pub async fn reset_docker_compose() {
    let output_down = std::process::Command::new("docker")
        .args(["compose", "-f", "docker-compose.dev.yml", "down"])
        .output()
        .expect("Failed to execute docker compose down");
    if !output_down.status.success() {
        panic!(
            "docker compose down failed: {}",
            String::from_utf8_lossy(&output_down.stderr)
        );
    }

    let output_up = std::process::Command::new("docker")
        .args([
            "compose",
            "-f",
            "docker-compose.dev.yml",
            "up",
            "-d",
            "--build",
        ])
        .output()
        .expect("Failed to execute docker compose up -d --build");
    if !output_up.status.success() {
        panic!(
            "docker compose up failed: {}",
            String::from_utf8_lossy(&output_up.stderr)
        );
    }

    println!("Checking SSH connectivity to vps1-3...");
    for port in [2221, 2222, 2223] {
        let mut attempt = 1;
        loop {
            let status = std::process::Command::new("ssh")
                .args([
                    "-i",
                    "keys/id_ed25519",
                    "-o",
                    "StrictHostKeyChecking=no",
                    "-o",
                    "UserKnownHostsFile=/dev/null",
                    "-o",
                    "ConnectTimeout=2",
                    "-p",
                    &port.to_string(),
                    "crane@localhost",
                    "true",
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();

            match status {
                Ok(status) if status.success() => {
                    break;
                }
                _ => {
                    if attempt >= 15 {
                        panic!(
                            "Failed to connect to vps on port {} after 15 attempts",
                            port
                        );
                    }
                    attempt += 1;
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }
    }
    println!("✓ Connection verification succeeded for all VPS instances");
}

pub fn run_sql(interactor: &dyn ServerInteractor, sql: &str) -> String {
    let cmd = format!("sudo -u postgres psql -d mydb -t -A -c {:?}", sql);
    let out = interactor.cmd(&cmd).expect("SQL execution failed");
    assert_eq!(out.exit_code, 0, "SQL failed: {}", out.stderr);
    out.stdout.trim().to_string()
}
