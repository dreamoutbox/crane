use crate::config;
use crate::server_interactor::SSHSession;
use crate::server_interactor::debian::DebianInteractor;
use crate::server_interactor::server_interactor_trait::ServerInteractor;
use std::path::Path;

/// deploy app commands
pub fn run(config_path: &Path) -> anyhow::Result<()> {
    println!("Loading configuration from {:?}", config_path);

    let config = config::load_config(config_path)?;
    let config_dir = config_path.parent().unwrap_or(Path::new("."));
    let env_path = config_dir.join(".env");
    let dot_env = config::load_env_file(&env_path).unwrap_or_default();

    // Get datetime for release
    let output = std::process::Command::new("date")
        .arg("+%Y%m%d_%H%M%S")
        .output()?;
    let datetime = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if datetime.is_empty() {
        anyhow::bail!("Failed to generate datetime prefix using 'date' command");
    }

    for (app_id, app) in &config.app {
        println!(
            "Starting deployment for app '{}' (ID: {})...",
            app.name, app_id
        );

        let binary_path = config_dir.join(&app.binary);
        if !binary_path.exists() {
            anyhow::bail!("Binary file not found at {:?}", binary_path);
        }

        // Merge environment
        let mut merged_env = std::collections::HashMap::new();
        if let Some(ref env_map) = app.env {
            for (k, v) in env_map {
                merged_env.insert(k.clone(), v.clone());
            }
        }
        for (k, v) in &dot_env {
            merged_env.insert(k.clone(), v.clone());
        }

        let mut env_content = String::new();
        for (k, v) in &merged_env {
            env_content.push_str(&format!("{}={}\n", k, v));
        }

        // Find app nodes
        let app_nodes: Vec<_> = config
            .nodes
            .iter()
            .filter(|n| n.roles.contains(&"app".to_string()))
            .collect();
        if app_nodes.is_empty() {
            anyhow::bail!("No nodes with the 'app' role found in configuration");
        }

        for node in app_nodes {
            println!(
                "Deploying to node: {}@{} (port: {})",
                node.user, node.host, node.port
            );
            let ssh = SSHSession::new(
                node.host.clone(),
                node.user.clone(),
                "".to_string(),
                Some(node.port),
            );
            let interactor = DebianInteractor::new(ssh);

            // 1. Setup user if specified
            if let Some(ref users) = config.users {
                if let Some(user_config) = users.iter().find(|u| u.name == app.deploy_user) {
                    let mut authorized_keys = Vec::new();
                    for key in &user_config.ssh_authorized_keys {
                        let expanded_path = if key.starts_with('~') {
                            if let Some(home) = std::env::var_os("HOME") {
                                Path::new(&home)
                                    .join(key.strip_prefix("~").unwrap().trim_start_matches('/'))
                            } else {
                                std::path::PathBuf::from(key)
                            }
                        } else {
                            std::path::PathBuf::from(key)
                        };
                        if let Ok(content) = std::fs::read_to_string(expanded_path) {
                            authorized_keys.push(content.trim().to_string());
                        } else {
                            authorized_keys.push(key.clone());
                        }
                    }
                    let register =
                        crate::server_interactor::server_interactor_trait::UserRegister::new(
                            user_config.name.clone(),
                            user_config.groups.clone(),
                            authorized_keys,
                        );
                    interactor.create_user(register)?;
                }
            }

            // 2. Install dependencies
            if let Some(ref deps) = app.dependencies {
                interactor.install_dependencies(deps.clone())?;
            }

            // 3. Prepare release directory and upload binary
            let release_dir = format!("/opt/{}/releases/{}", app.name, datetime);
            interactor.cmd(&format!("sudo mkdir -p '{}'", release_dir))?;

            let temp_remote_path = format!("/tmp/{}-{}", app.name, datetime);
            interactor.upload(binary_path.to_str().unwrap(), &temp_remote_path)?;

            let final_remote_path = format!("{}/{}", release_dir, app.name);
            interactor.cmd(&format!(
                "sudo mv '{}' '{}'",
                temp_remote_path, final_remote_path
            ))?;
            interactor.cmd(&format!("sudo chmod +x '{}'", final_remote_path))?;
            interactor.cmd(&format!(
                "sudo chown -R '{}:{}' '/opt/{}'",
                app.deploy_user, app.deploy_user, app.name
            ))?;

            // 4. Rolling deploy across instances
            let port_end = app.port_start + app.instances as u16;
            for port in app.port_start..port_end {
                println!("Deploying instance of '{}' on port {}...", app.name, port);
                let service_instance = format!("{}@{}", app.name, port);

                // Stop service if running
                let _ = interactor.stop_service(&service_instance);

                // Update current symlink
                interactor.cmd(&format!(
                    "sudo ln -sfn '{}/{}' '/opt/{}/current'",
                    release_dir, app.name, app.name
                ))?;

                // Write environment file
                let env_dir = format!("/etc/crane/{}", app.name);
                interactor.cmd(&format!("sudo mkdir -p '{}'", env_dir))?;
                let temp_env_path = format!("/tmp/crane-env-{}", port);
                interactor.create_file(&temp_env_path, &env_content)?;
                let final_env_path = format!("{}/.env", env_dir);
                interactor.cmd(&format!("sudo mv '{}' '{}'", temp_env_path, final_env_path))?;
                interactor.cmd(&format!(
                    "sudo chown '{}:{}' '{}'",
                    app.deploy_user, app.deploy_user, final_env_path
                ))?;
                interactor.cmd(&format!("sudo chmod 600 '{}'", final_env_path))?;

                // Create systemd template unit
                let systemd_template = format!(
                    "[Unit]\nDescription=crane managed: %p instance on port %i\nAfter=network.target\n\n[Service]\nType=simple\nUser={deploy_user}\nWorkingDirectory=/opt/{appname}\nExecStart=/opt/{appname}/current --port %i\nEnvironmentFile=/etc/crane/{appname}/.env\nRestart=on-failure\nRestartSec=5\nNoNewPrivileges=true\nProtectSystem=strict\nProtectHome=true\nPrivateTmp=true\n\n[Install]\nWantedBy=multi-user.target\n",
                    deploy_user = app.deploy_user,
                    appname = app.name,
                );
                let temp_service_path = format!("/tmp/{}@.service", app.name);
                interactor.create_file(&temp_service_path, &systemd_template)?;
                let dest_service_path = format!("/etc/systemd/system/{}@.service", app.name);
                interactor.cmd(&format!(
                    "sudo mv '{}' '{}'",
                    temp_service_path, dest_service_path
                ))?;
                interactor.cmd(&format!("sudo chown root:root '{}'", dest_service_path))?;
                interactor.cmd(&format!("sudo chmod 644 '{}'", dest_service_path))?;
                interactor.cmd("sudo systemctl daemon-reload")?;
                let _ = interactor.cmd(&format!("sudo systemctl enable '{}@'", app.name));

                // Start service
                interactor.cmd(&format!("sudo systemctl start '{}'", service_instance))?;

                // Health check loop
                let health_path = app.health_check_path.as_deref().unwrap_or("/health");
                let timeout_secs = app.health_check_timeout.unwrap_or(30);
                let interval_secs = app.health_check_interval.unwrap_or(2);

                println!("Polling health check for {} on port {}...", app.name, port);
                let mut healthy = false;
                let start_time = std::time::Instant::now();
                while start_time.elapsed().as_secs() < timeout_secs {
                    let curl_cmd = format!(
                        "curl -s -o /dev/null -w \"%{{http_code}}\" http://127.0.0.1:{}{}",
                        port, health_path
                    );
                    if let Ok(code) = interactor.cmd(&curl_cmd) {
                        if code.trim() == "200" {
                            healthy = true;
                            break;
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_secs(interval_secs));
                }

                if !healthy {
                    anyhow::bail!(
                        "Health check failed for {} on port {} within {} seconds",
                        app.name,
                        port,
                        timeout_secs
                    );
                }
                println!("Instance on port {} is healthy!", port);
            }

            // 5. Write Traefik dynamic config
            let domain = app.domain.clone().unwrap_or_else(|| {
                config
                    .domain
                    .as_ref()
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| "localhost".to_string())
            });
            crate::traefik_unit::setup::setup_traefik(
                &interactor,
                &app.name,
                &domain,
                app.port_start,
                port_end,
            )?;

            // 6. Prune old releases
            let retain = app.retain_releases.unwrap_or(3) as usize;
            let releases_dir = format!("/opt/{}/releases", app.name);
            if let Ok(list_output) = interactor.cmd(&format!("ls -1d {}/*", releases_dir)) {
                let mut dirs: Vec<String> = list_output
                    .lines()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                dirs.sort();
                if dirs.len() > retain {
                    let to_remove = dirs.len() - retain;
                    for dir in dirs.iter().take(to_remove) {
                        println!("Pruning old release: {}", dir);
                        let _ = interactor.cmd(&format!("sudo rm -rf '{}'", dir));
                    }
                }
            }
        }
    }

    Ok(())
}
