use crate::{
    helper::config::config_get_nodes,
    postgres_unit::{
        entity::{HAProxyNode, PostgresNode, PostgresStatusOutput},
        helper::{connect_to_node, get_pg_version},
    },
};

pub async fn get_postgres_status_wrapper(
    config: &crate::config::Config,
) -> anyhow::Result<PostgresStatusOutput> {
    let pg_nodes = config_get_nodes(&config, "postgres");

    let app_nodes = config_get_nodes(&config, "app");

    let pg_version = get_pg_version(&config);

    let mut pgs = Vec::new();
    let mut pg_primary_name = "Unknown".to_string();

    let mut haproxy_status = "Unhealthy".to_string();
    let mut haproxy_active_count = 0;

    let mut handles = vec![];

    for node in &pg_nodes {
        let node = node.clone();
        let pg_version = pg_version.clone();
        let config = config.clone();

        let handle = tokio::task::spawn_blocking(move || -> (PostgresNode, bool) {
            let mut role = "Unknown".to_string();
            let mut version = pg_version;
            let mut status = "Unhealthy".to_string();
            let mut haproxy_active = false;

            match connect_to_node(&node, &config) {
                Ok(interactor) => {
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

                    if let Ok(started) = interactor.wait_for_service_start("haproxy", 10) {
                        haproxy_active = started
                    }
                }

                Err(_) => {
                    // SSH connection failure defaults to Unhealthy
                }
            }

            (
                PostgresNode {
                    node,
                    version,
                    status,
                    role,
                },
                haproxy_active,
            )
        });

        handles.push(handle);
    }

    for handle in handles {
        let (pg_node, is_haproxy_active) = handle.await?;

        if pg_node.role == "Leader" {
            pg_primary_name = pg_node.node.name.clone();
        }
        if is_haproxy_active {
            haproxy_active_count += 1;
        }

        pgs.push(pg_node);
    }

    if haproxy_active_count == app_nodes.len() {
        haproxy_status = "Healthy".to_string();
    } else if haproxy_active_count > 0 {
        haproxy_status = "Degraded".to_string();
    }

    // Identify replicas (all postgres nodes that are not the leader)
    let mut replicas = Vec::new();
    for pg_node in &pgs {
        if pg_node.node.name != pg_primary_name {
            replicas.push(format!("{}:5000", pg_node.node.name));
        }
    }

    let status_output = PostgresStatusOutput {
        haproxy: HAProxyNode {
            status: haproxy_status,
            primary: pg_primary_name,
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
        println!("\n{}", status.node.name);
        println!("Address: {}:{}", status.node.ssh_ip, status.node.ssh_port);
        println!("Role: {}", status.role);
        println!("DB version: {}", status.version);
        println!("Status: {}", status.status);
    }
    println!("");

    Ok(())
}
