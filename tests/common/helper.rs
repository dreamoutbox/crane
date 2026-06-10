use crane::{postgres_unit::helper::pg_get_primary, server_interactor::get_server_interactor};

pub async fn reset_docker_compose() {
    let docker_check = std::process::Command::new("docker").arg("info").output();

    let docker_available = match docker_check {
        Ok(output) => output.status.success(),
        Err(_) => false,
    };

    if !docker_available {
        println!(
            "Docker daemon is not running or docker is not installed. Skipping docker-dependent test."
        );
        std::process::exit(0);
    }

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
        .args(["compose", "-f", "docker-compose.dev.yml", "up", "-d"])
        .output()
        .expect("Failed to execute docker compose up -d");

    if !output_up.status.success() {
        panic!(
            "docker compose up failed: {}",
            String::from_utf8_lossy(&output_up.stderr)
        );
    }

    for node in ["vps1", "vps2", "vps3"] {
        let mut attempt = 1;
        loop {
            let output_cp = std::process::Command::new("docker")
                .args([
                    "exec",
                    node,
                    "sh",
                    "-c",
                    "cp /opt/authorized_keys /home/crane/.ssh/authorized_keys && chown crane:crane /home/crane/.ssh/authorized_keys && chmod 600 /home/crane/.ssh/authorized_keys",
                ])
                .output()
                .expect("Failed to execute docker exec to setup ssh key");

            if output_cp.status.success() {
                break;
            }

            if attempt >= 5 {
                panic!(
                    "Failed to setup SSH key in container {} after 5 attempts.\nSTDOUT:\n{}\nSTDERR:\n{}",
                    node,
                    String::from_utf8_lossy(&output_cp.stdout),
                    String::from_utf8_lossy(&output_cp.stderr)
                );
            }

            attempt += 1;
            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        }
    }

    println!("Checking SSH connectivity to vps1-3...");
    for port in [2221, 2222, 2223] {
        let mut attempt = 1;

        loop {
            let output = std::process::Command::new("ssh")
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
                .output();

            match output {
                Ok(output) if output.status.success() => {
                    break;
                }
                Ok(output) => {
                    eprintln!(
                        "SSH connection attempt {} to port {} failed:\n--- stdout ---\n{}\n--- stderr ---\n{}",
                        attempt,
                        port,
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    );
                    if attempt >= 15 {
                        panic!(
                            "Failed to connect to vps on port {} after 15 attempts",
                            port
                        );
                    }

                    attempt += 1;
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
                Err(e) => {
                    eprintln!("Failed to run ssh command: {:?}", e);
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

    println!("Connection to all VPS ready");
}

pub fn pg_allow_host_machine(config: &crane::config::Config) {
    let primary_node = pg_get_primary(config)
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
