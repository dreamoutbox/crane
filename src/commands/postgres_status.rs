use crate::{
    helper::config::config_get_nodes,
    postgres_unit::{
        entity::{HAProxyNode, PostgresNode, PostgresStatusOutput},
        helper::get_pg_version,
        migration::detect_node_postgres_mode,
        postgres_deployment_mode::PostgresDeploymentMode,
    },
    server_interactor::get_server_interactor,
};

/// Gathers status for the PostgreSQL deployment across all configured postgres nodes,
/// including deployed topology mode (minimal vs cluster), HAProxy connectivity, and per-node roles.
///
/// # Arguments
/// * `config` - The loaded configuration containing node definitions and postgres settings.
///
/// # Returns
/// A `PostgresStatusOutput` containing the detected deployment mode, optional warning
/// if live deployment disagrees with configuration, HAProxy status, and per-node details.
///
/// # Example
/// ```no_run
/// use crane::config::Config;
/// use crane::commands::postgres_status::get_postgres_status_wrapper;
///
/// # async fn check(cfg: &Config) -> anyhow::Result<()> {
/// let status = get_postgres_status_wrapper(cfg).await?;
/// println!("Mode: {}", status.mode.as_str());
/// # Ok(())
/// # }
/// ```
pub async fn get_postgres_status_wrapper(
    config: &crate::config::Config,
) -> anyhow::Result<PostgresStatusOutput> {
    let pg_nodes = config_get_nodes(config, "postgres");
    let app_nodes = config_get_nodes(config, "app");
    let pg_version = get_pg_version(config);

    let mut pgs = Vec::new();
    let mut pg_primary_name = "Unknown".to_string();

    let mut haproxy_status = "Unhealthy".to_string();
    let mut haproxy_active_count = 0;

    let mut handles = vec![];

    for node in &pg_nodes {
        let node = node.clone();
        let pg_version = pg_version.clone();

        let handle = tokio::task::spawn_blocking(
            move || -> (PostgresNode, bool, PostgresDeploymentMode) {
                let mut role = "Unknown".to_string();
                let mut version = pg_version.clone();
                let mut status = "Unhealthy".to_string();
                let mut haproxy_active = false;
                let mut detected_mode = PostgresDeploymentMode::Uninstalled;

                match get_server_interactor(&node.name) {
                    Ok(interactor) => {
                        detected_mode = detect_node_postgres_mode(&*interactor, &version);

                        let is_recovery =
                            interactor.psql(Some("select pg_is_in_recovery();"), None, None, true);
                        let db_ver_str =
                            interactor.psql(Some("show server_version;"), None, None, true);

                        if let Ok(rec) = is_recovery {
                            let rec_trimmed = rec.stdout.trim();
                            if detected_mode == PostgresDeploymentMode::Minimal {
                                if rec_trimmed == "f" {
                                    role = "Standalone".to_string();
                                    status = "Healthy".to_string();
                                }
                            } else if rec_trimmed == "f" {
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
                            haproxy_active = started;
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
                    detected_mode,
                )
            },
        );

        handles.push(handle);
    }

    let mut overall_mode = PostgresDeploymentMode::Uninstalled;

    for handle in handles {
        let (pg_node, is_haproxy_active, node_mode) = handle.await?;

        if node_mode == PostgresDeploymentMode::Cluster {
            overall_mode = PostgresDeploymentMode::Cluster;
        } else if node_mode == PostgresDeploymentMode::Minimal
            && overall_mode != PostgresDeploymentMode::Cluster
        {
            overall_mode = PostgresDeploymentMode::Minimal;
        }

        if pg_node.role == "Leader" || pg_node.role == "Standalone" {
            pg_primary_name = pg_node.node.name.clone();
        }
        if is_haproxy_active {
            haproxy_active_count += 1;
        }

        pgs.push(pg_node);
    }

    let config_is_minimal = crate::config::postgres_is_minimal(config);
    let target_mode = if config_is_minimal {
        PostgresDeploymentMode::Minimal
    } else {
        PostgresDeploymentMode::Cluster
    };

    let warning = if overall_mode == PostgresDeploymentMode::Uninstalled {
        if !pg_nodes.is_empty() {
            Some("Warning: PostgreSQL is not detected on target node(s)".to_string())
        } else {
            None
        }
    } else if overall_mode != target_mode {
        Some(format!(
            "Warning: Deployed mode ({}) differs from config setting (minimal = {})",
            overall_mode.as_str(),
            config_is_minimal
        ))
    } else {
        None
    };

    if haproxy_active_count == app_nodes.len() {
        haproxy_status = "Healthy".to_string();
    } else if haproxy_active_count > 0 {
        haproxy_status = "Degraded".to_string();
    }

    // Identify replicas (all postgres nodes that are not the leader/primary)
    let mut replicas = Vec::new();
    for pg_node in &pgs {
        if pg_node.node.name != pg_primary_name {
            replicas.push(format!("{}:5000", pg_node.node.name));
        }
    }

    let status_output = PostgresStatusOutput {
        mode: overall_mode,
        warning,
        haproxy: HAProxyNode {
            status: haproxy_status,
            primary: pg_primary_name,
            replicas,
        },
        postgres: pgs,
    };

    Ok(status_output)
}

/// Runs the `postgres status` CLI command, displaying the PostgreSQL deployment mode,
/// topology mismatch warnings, HAProxy routing status, and per-node health information.
///
/// # Arguments
/// * `config` - The loaded configuration containing node definitions and postgres settings.
///
/// # Example
/// ```no_run
/// use crane::config::Config;
/// use crane::commands::postgres_status::run_postgres_status_command;
///
/// # async fn run(cfg: &Config) -> anyhow::Result<()> {
/// run_postgres_status_command(cfg).await?;
/// # Ok(())
/// # }
/// ```
pub async fn run_postgres_status_command(config: &crate::config::Config) -> anyhow::Result<()> {
    let status = get_postgres_status_wrapper(config).await?;

    println!("\nMode: {}", status.mode.as_str());
    if let Some(ref warning) = status.warning {
        println!("{}", warning);
    }

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
