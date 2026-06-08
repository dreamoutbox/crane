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

    println!("Connection to all VPS ready");
}
