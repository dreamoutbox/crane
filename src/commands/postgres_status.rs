use crate::postgres_unit::{
    entity::{HAProxyNode, PostgresNode, PostgresStatusOutput},
    helper::connect_to_node,
};

pub async fn get_postgres_status_wrapper(
    config: &crate::config::Config,
) -> anyhow::Result<PostgresStatusOutput> {
    let pg_nodes: Vec<_> = config
        .nodes
        .iter()
        .filter(|n| n.roles.contains(&"postgres".to_string()))
        .cloned()
        .collect();

    let app_nodes: Vec<_> = config
        .nodes
        .iter()
        .filter(|n| n.roles.contains(&"app".to_string()))
        .cloned()
        .collect();

    let pg_version_config = config
        .db
        .as_ref()
        .and_then(|db| db.postgres.as_ref())
        .map(|pg| pg.version.as_str())
        .unwrap_or("16")
        .to_string();

    let mut pgs = Vec::new();
    let mut pg_primary_host = "Unknown".to_string();

    let mut haproxy_status = "Unhealthy".to_string();
    let mut haproxy_active_count = 0;

    let mut handles = vec![];

    for node in &pg_nodes {
        let node = node.clone();
        let config = config.clone();
        let pg_version_config = pg_version_config.clone();

        let handle = tokio::task::spawn_blocking(move || -> (PostgresNode, bool) {
            let address = format!("{}:{}", node.public_ip, node.port);
            let mut hostname = node.host.clone();
            let mut role = "Unknown".to_string();
            let mut version = pg_version_config;
            let mut status = "Unhealthy".to_string();
            let mut haproxy_active = false;

            match connect_to_node(&node, &config) {
                Ok(interactor) => {
                    // 1. Get Hostname
                    if let Ok(h) = interactor.cmd("hostname") {
                        let h_trimmed = h.stdout.trim();
                        if !h_trimmed.is_empty() {
                            hostname = h_trimmed.to_string();
                        }
                    }

                    // 2. Check Recovery & DB Version
                    let recovery_cmd =
                        r#"sudo -u postgres psql -t -A -c "select pg_is_in_recovery();""#;
                    let version_cmd = r#"sudo -u postgres psql -t -A -c "show server_version;""#;

                    let is_recovery = interactor.cmd(recovery_cmd);
                    let db_ver_str = interactor.cmd(version_cmd);

                    if let Ok(rec) = is_recovery {
                        let rec_trimmed = rec.stdout.trim();
                        if rec_trimmed == "f" {
                            role = "Leader".to_string();
                            status = "Healthy".to_string();
                        } else if rec_trimmed == "t" {
                            role = "Follower".to_string();
                            status = "Healthy".to_string();
                        }
                    }

                    if let Ok(v_str) = db_ver_str {
                        let v_trimmed = v_str.stdout.trim();
                        if let Some(major) = v_trimmed.split('.').next() {
                            let major_clean = major.trim();
                            if !major_clean.is_empty() {
                                version = major_clean.to_string();
                            }
                        }
                    }

                    if let Ok(output) = interactor.cmd("systemctl is-active haproxy") {
                        if output.stdout.trim() == "active" {
                            haproxy_active = true;
                        }
                    }
                }

                Err(_) => {
                    // SSH connection failure defaults to Unhealthy
                }
            }

            (
                PostgresNode {
                    hostname,
                    address,
                    role,
                    version,
                    status,
                },
                haproxy_active,
            )
        });

        handles.push(handle);
    }

    for handle in handles {
        let (node_info, is_haproxy_active) = handle.await?;
        if node_info.role == "Leader" {
            pg_primary_host = node_info.hostname.clone();
        }
        if is_haproxy_active {
            haproxy_active_count += 1;
        }
        pgs.push(node_info);
    }

    if haproxy_active_count == app_nodes.len() {
        haproxy_status = "Healthy".to_string();
    } else if haproxy_active_count > 0 {
        haproxy_status = "Degraded".to_string();
    }

    // Identify replicas (all postgres nodes that are not the leader)
    let mut replicas = Vec::new();
    for status in &pgs {
        if status.hostname != pg_primary_host {
            replicas.push(format!("{}:5000", status.hostname));
        }
    }

    let status_output = PostgresStatusOutput {
        haproxy: HAProxyNode {
            status: haproxy_status,
            primary: pg_primary_host,
            replicas,
        },
        postgres: pgs,
    };

    Ok(status_output)
}

pub async fn run_postgres_status_command(config: &crate::config::Config) -> anyhow::Result<()> {
    let status = get_postgres_status_wrapper(config).await?;

    // Print expected output format
    println!("\nHAProxy");
    println!("Primary: {}:5000", status.haproxy.primary);
    println!("Replicas: {}", status.haproxy.replicas.join(","));
    println!("Status: {}", status.haproxy.status);

    for status in &status.postgres {
        println!("\n{}", status.hostname);
        println!("Address: {}", status.address);
        println!("Role: {}", status.role);
        println!("DB version: {}", status.version);
        println!("Status: {}", status.status);
    }
    println!("");

    Ok(())
}
