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
                    "cp /tmp/authorized_keys /home/crane/.ssh/authorized_keys && chown crane:crane /home/crane/.ssh/authorized_keys && chmod 600 /home/crane/.ssh/authorized_keys",
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
