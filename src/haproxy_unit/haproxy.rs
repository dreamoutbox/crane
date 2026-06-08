use crate::{
    config,
    server_interactor::{get_server_interactor, server_interactor_trait::ServerInteractor},
};

pub fn install_haproxy(interactor: &dyn ServerInteractor) -> anyhow::Result<()> {
    let haproxy_installed = interactor.which("haproxy").is_ok();

    // Check if HAProxy binary is already installed
    if !haproxy_installed {
        println!("\tInstalling HAProxy on remote server...");
        interactor.install_dependencies(vec!["haproxy".to_string()])?;
    }

    Ok(())
}

fn ensure_self_signed_cert(interactor: &dyn ServerInteractor) -> anyhow::Result<String> {
    let cert_dir = "/etc/ssl/private";
    let cert_path = "/etc/ssl/private/crane_self_signed.pem";
    let key_path = "/etc/ssl/private/crane_self_signed.key";
    let crt_path = "/etc/ssl/private/crane_self_signed.crt";

    // Ensure cert directory exists
    interactor.mkdir(cert_dir)?;

    // Check if the pem file exists
    if !interactor.exists(cert_path)? {
        println!("\tGenerating self-signed certificate for SSL/TLS termination...");
        interactor.generate_self_signed_cert(key_path, crt_path, cert_path)?;
    }

    Ok(cert_path.to_string())
}

pub fn setup_haproxy_unified(
    interactor: &dyn ServerInteractor,
    config: &config::Config,
    node: &config::NodeConfig,
    current_app_name: Option<&str>,
    current_port_end: Option<u16>,
) -> anyhow::Result<()> {
    let haproxy_config_path = "/etc/haproxy/haproxy.cfg";

    println!(
        "\tUpdate HAProxy config on node {} (at: {})",
        node.name, haproxy_config_path
    );

    // 1. Base Global and Defaults configuration
    let mut haproxy_cfg = format!(
        r#"global
    log /dev/log local0
    log /dev/log local1 notice
    chroot /var/lib/haproxy
    user haproxy
    group haproxy
    daemon

defaults
    log global
    mode http
    option httplog
    option dontlognull
    retries 3
    timeout connect 5000ms
    timeout client 50000ms
    timeout server 50000ms
"#
    );

    // 2. Add PostgreSQL clustering routing if enabled
    let pg_enabled = config
        .db
        .as_ref()
        .and_then(|db| db.postgres.as_ref())
        .map(|pg| pg.enabled)
        .unwrap_or(false);

    if pg_enabled {
        // Try to dynamically get postgres leader
        let leader_node = match crate::postgres_unit::helper::pg_get_primary(config) {
            Ok(Some(n)) => Some(n),
            _ => {
                // Fallback to first node with postgres role
                config
                    .nodes
                    .iter()
                    .find(|n| n.roles.contains(&"postgres".to_string()))
                    .cloned()
            }
        };

        if let Some(leader) = leader_node {
            let follower_ips: Vec<String> = config
                .nodes
                .iter()
                .filter(|n| {
                    n.roles.contains(&"postgres".to_string()) && n.internal_ip != leader.internal_ip
                })
                .map(|n| n.internal_ip.clone())
                .collect();

            let mut pg_ips = vec![leader.internal_ip.clone()];
            pg_ips.extend(follower_ips);

            haproxy_cfg.push_str(
                r#"
frontend postgres_primary_front
    bind *:5000
    mode tcp
    option tcplog
    default_backend postgres_primary_back

backend postgres_primary_back
    mode tcp
    option httpchk GET /primary
    http-check expect status 200
    default-server inter 3s fall 3 rise 2 check port 8008 on-marked-down shutdown-sessions
"#,
            );

            for (idx, ip) in pg_ips.iter().enumerate() {
                haproxy_cfg.push_str(&format!(
                    "    server postgres-node-{} {}:5432 check\n",
                    idx + 1,
                    ip
                ));
            }

            haproxy_cfg.push_str(
                r#"
frontend postgres_replica_front
    bind *:5001
    mode tcp
    option tcplog
    default_backend postgres_replica_back

backend postgres_replica_back
    mode tcp
    balance roundrobin
    option httpchk GET /replica
    http-check expect status 200
    default-server inter 3s fall 3 rise 2 check port 8008
"#,
            );

            for (idx, ip) in pg_ips.iter().enumerate() {
                haproxy_cfg.push_str(&format!(
                    "    server postgres-node-{} {}:5432 check\n",
                    idx + 1,
                    ip
                ));
            }
        }
    }

    // 3. Add Application HTTP/HTTPS routing
    if !config.app.is_empty() {
        // Collect SSL cert configurations
        let fallback_cert = ensure_self_signed_cert(interactor)?;
        let mut cert_args = format!("crt {}", fallback_cert);

        let mut custom_certs = Vec::new();
        if let Some(ref dom) = config.domain {
            if let Some(ref cert) = dom.ssl_cert {
                custom_certs.push(cert.clone());
            }
        }
        for app in config.app.values() {
            if let Some(ref cert) = app.ssl_cert {
                custom_certs.push(cert.clone());
            }
        }

        custom_certs.sort();
        custom_certs.dedup();

        for cert in custom_certs {
            if interactor.exists(&cert)? {
                cert_args.push_str(&format!(" crt {}", cert));
            } else {
                println!(
                    "\tWarning: Custom SSL certificate '{}' not found on node. Skipping.",
                    cert
                );
            }
        }

        // Generate frontends
        haproxy_cfg.push_str(
            "\nfrontend http_front\n    bind *:80\n    bind 127.0.0.1:8080\n    mode http\n",
        );

        // Internal bypass rules
        for app in config.app.values() {
            haproxy_cfg.push_str(&format!(
                "    acl is_internal_{name} hdr(host) -i {name}\n    use_backend {name}_backend if is_internal_{name}\n",
                name = app.name
            ));
        }

        // Redirect HTTP to HTTPS unless it is an internal host
        let mut unless_clause = String::new();
        for (idx, app) in config.app.values().enumerate() {
            if idx > 0 {
                unless_clause.push_str(" || ");
            }
            unless_clause.push_str(&format!("is_internal_{}", app.name));
        }
        if !unless_clause.is_empty() {
            haproxy_cfg.push_str(&format!(
                "    http-request redirect scheme https unless {}\n",
                unless_clause
            ));
        } else {
            haproxy_cfg.push_str("    http-request redirect scheme https\n");
        }

        haproxy_cfg.push_str(&format!(
            "\nfrontend https_front\n    bind *:443 ssl {}\n    mode http\n    option forwardfor\n    http-request set-header X-Forwarded-Proto https\n",
            cert_args
        ));

        // External domain routing rules - using starts_with matching for robust tests
        for app in config.app.values() {
            haproxy_cfg.push_str(&format!(
                "    acl host_{name} hdr(host) -i {name}\n    acl host_{name}_sub hdr_beg(host) -i {name}.\n    use_backend {name}_backend if host_{name} || host_{name}_sub\n",
                name = app.name
            ));
        }

        // Backend server pools
        for app in config.app.values() {
            let port_end = if Some(app.name.as_str()) == current_app_name {
                current_port_end.unwrap_or(app.port_start + app.instances as u16)
            } else {
                app.port_start + app.instances as u16
            };

            let health_path = app.health_check_path.as_deref().unwrap_or("/health");

            haproxy_cfg.push_str(&format!(
                r#"
backend {name}_backend
    mode http
    balance roundrobin
    option httpchk GET {health_path}
    default-server inter 2s fall 3 rise 2
"#,
                name = app.name,
                health_path = health_path
            ));

            for port in app.port_start..port_end {
                haproxy_cfg.push_str(&format!(
                    "    server {name}-port-{port} 127.0.0.1:{port} check\n",
                    name = app.name,
                    port = port
                ));
            }
        }
    }

    // Write config to remote node
    interactor.create_file(haproxy_config_path, &haproxy_cfg)?;
    interactor.chown(haproxy_config_path, "root", "root")?;
    interactor.chmod(haproxy_config_path, "644")?;

    // Write to local file for debug
    std::fs::write(format!("haproxy_node_{}.cfg", node.name), &haproxy_cfg)?;

    Ok(())
}

pub fn reload_haproxy(interactor: &dyn ServerInteractor) -> anyhow::Result<()> {
    interactor.service_daemon_reload()?;
    interactor.enable_service("haproxy")?;
    interactor.restart_service("haproxy")?;
    Ok(())
}

pub async fn setup_haproxy_on_each_nodes_wrapper(
    config: &config::Config,
    app_nodes: &Vec<config::NodeConfig>,
) -> Result<(), anyhow::Error> {
    let mut handles = vec![];
    for app_node in app_nodes {
        let app_node = app_node.clone();
        let config = config.clone();

        let handle = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            // println!("\n\tSetting up HAProxy on app node {}...", app_node.name);

            let interactor = get_server_interactor(&app_node.name)?;

            setup_haproxy_unified(&*interactor, &config, &app_node, None, None)?;

            Ok(())
        });

        handles.push(handle);
    }

    let mut results = vec![];

    for handle in handles {
        results.push(handle.await);
    }

    for res in results {
        res??;
    }

    Ok(())
}

pub async fn reload_haproxy_on_each_nodes_wrapper(
    app_nodes: &Vec<config::NodeConfig>,
) -> Result<(), anyhow::Error> {
    let mut handles = vec![];

    for app_node in app_nodes {
        let app_node = app_node.clone();
        let handle = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            println!("\tReloading HAProxy on app node {}...", app_node.name);

            let interactor = get_server_interactor(&app_node.name)?;

            reload_haproxy(&*interactor)?;

            Ok(())
        });
        handles.push(handle);
    }

    let mut results = vec![];
    for handle in handles {
        results.push(handle.await);
    }
    for res in results {
        res??;
    }

    Ok(())
}
