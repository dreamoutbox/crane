use crate::config;
use crate::helper::keys::{find_private_key_for_user, get_any_private_key};
use crate::postgres_unit::setup::postgres_setup_wrapper;
use crate::server_interactor::server_interactor_trait::ServerInteractor;
use crate::ssh::SSHSession;
use std::path::Path;

/// deploy app commands
pub fn run(
    config_path: &Path,
    get_interactor: fn(SSHSession) -> anyhow::Result<Box<dyn ServerInteractor>>,
) -> anyhow::Result<()> {
    println!("Loading configuration from {:?}\n\n", config_path);

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

    // Collect all dependencies across all apps (deduped), then install once per node
    let mut all_deps: Vec<String> = vec!["unzip".to_string()];
    for (_, app) in &config.app {
        if let Some(deps) = &app.dependencies {
            for d in deps {
                if !all_deps.contains(d) {
                    all_deps.push(d.clone());
                }
            }
        }
    }

    let app_nodes: Vec<_> = config
        .nodes
        .iter()
        .filter(|n| n.roles.contains(&"app".to_string()))
        .cloned()
        .collect();

    for node in &app_nodes {
        println!(
            "Installing dependencies on {}@{} (port: {})...",
            node.user, node.host, node.port
        );
        let private_key = find_private_key_for_user(&node.user, &config);
        let private_key = if private_key.is_empty() {
            get_any_private_key(&config)
        } else {
            private_key
        };
        let ssh = SSHSession::new(
            node.host.clone(),
            node.user.clone(),
            private_key,
            Some(node.port),
        );
        let interactor = get_interactor(ssh)?;
        interactor.install_dependencies(all_deps.clone())?;

        // install traefik
        crate::traefik_unit::setup::install_traefik(&*interactor)?;
    }

    // Install postgres database cluster if enabled
    let pg_enabled = config
        .db
        .as_ref()
        .and_then(|db| db.postgres.as_ref())
        .and_then(|pg| pg.get("enabled"))
        .and_then(|val| val.as_bool())
        .unwrap_or(false);

    if pg_enabled {
        postgres_setup_wrapper(get_interactor, &config, &dot_env, app_nodes)?;
    }

    let mut handles = vec![];

    // Loop Apps in Config and deploy in parallel using threads
    for (app_id, app) in config.app.clone() {
        let config_dir = config_dir.to_path_buf();
        let dot_env = dot_env.clone();
        let config = config.clone();
        let datetime = datetime.clone();

        let handle = std::thread::spawn(move || -> anyhow::Result<()> {
            println!(
                "Starting deployment for app '{}' (ID: {})...",
                app.name, app_id
            );

            let deploy_dir_candidate = config_dir.join(&app.deploy_dir);
            let deploy_dir_candidate = if deploy_dir_candidate.exists() {
                deploy_dir_candidate
            } else {
                Path::new(&app.deploy_dir).to_path_buf()
            };
            if !deploy_dir_candidate.exists() || !deploy_dir_candidate.is_dir() {
                anyhow::bail!(
                    "Deploy directory not found at {:?} or {:?}",
                    config_dir.join(&app.deploy_dir),
                    Path::new(&app.deploy_dir)
                );
            }
            // Canonicalize to absolute path so python script works regardless of CWD
            let deploy_dir = deploy_dir_candidate.canonicalize()?;

            let zip_path =
                std::env::temp_dir().join(format!("crane-deploy-{}-{}.zip", app.name, datetime));

            // Read .craneignore if it exists in the deploy_dir
            let craneignore_path = deploy_dir.join(".craneignore");
            let mut ignores = vec![];
            if craneignore_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&craneignore_path) {
                    for line in content.lines() {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() && !trimmed.starts_with('#') {
                            ignores.push(trimmed.to_string());
                        }
                    }
                }
            }
            ignores.push(".git".to_string());

            let python_code = r#"
import os, sys, zipfile
zip_path = sys.argv[1]
deploy_dir = sys.argv[2]
ignores = sys.argv[3:]
with zipfile.ZipFile(zip_path, 'w', zipfile.ZIP_DEFLATED) as zipf:
    for root, dirs, files in os.walk(deploy_dir):
        dirs[:] = [d for d in dirs if d not in ignores and not d.startswith('.')]
        for file in files:
            if file.startswith('.'):
                continue
            file_path = os.path.join(root, file)
            rel_path = os.path.relpath(file_path, deploy_dir)
            is_ignored = False
            for ig in ignores:
                if rel_path == ig or rel_path.startswith(ig + os.sep) or os.path.basename(rel_path) == ig:
                    is_ignored = True
                    break
            if not is_ignored:
                zipf.write(file_path, rel_path)
"#;
            let python_script_path =
                std::env::temp_dir().join(format!("crane-zip-helper-{}.py", datetime));
            std::fs::write(&python_script_path, python_code)?;

            let mut zip_cmd = std::process::Command::new("python3");
            zip_cmd
                .arg(&python_script_path)
                .arg(&zip_path)
                .arg(&deploy_dir);
            for ignore in ignores {
                zip_cmd.arg(ignore);
            }

            let status = zip_cmd.status()?;
            let _ = std::fs::remove_file(&python_script_path);

            if !status.success() {
                anyhow::bail!("Failed to create zip archive of {:?}", deploy_dir);
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
                .cloned()
                .collect();
            if app_nodes.is_empty() {
                anyhow::bail!("No nodes with the 'app' role found in configuration");
            }

            for node in app_nodes {
                println!(
                    "Deploying to node: {}@{} (port: {})",
                    node.user, node.host, node.port
                );

                let private_key = find_private_key_for_user(&node.user, &config);
                let private_key = if private_key.is_empty() {
                    get_any_private_key(&config)
                } else {
                    private_key
                };
                let ssh = SSHSession::new(
                    node.host.clone(),
                    node.user.clone(),
                    private_key,
                    Some(node.port),
                );
                let node_distro = crate::helper::server::get_server_distro(&ssh)?;
                let interactor =
                    crate::server_interactor::get_interactor_for_distro(ssh, &node_distro)?;

                // 1. Setup user if specified
                if let Some(ref users) = config.users {
                    if let Some(user_config) = users.iter().find(|u| u.name == app.deploy_user) {
                        let mut authorized_keys = Vec::new();
                        for key in &user_config.ssh_authorized_keys {
                            let expanded_path = if key.starts_with('~') {
                                if let Some(home) = std::env::var_os("HOME") {
                                    Path::new(&home).join(
                                        key.strip_prefix("~").unwrap().trim_start_matches('/'),
                                    )
                                } else {
                                    std::path::PathBuf::from(key)
                                }
                            } else {
                                std::path::PathBuf::from(key)
                            };

                            let mut key_content = None;
                            if let Ok(content) = std::fs::read_to_string(&expanded_path) {
                                key_content = Some(content);
                            } else if key.contains("id_rsa.pub") {
                                let fallback_path = expanded_path.with_file_name("id_ed25519.pub");
                                if let Ok(content) = std::fs::read_to_string(fallback_path) {
                                    key_content = Some(content);
                                }
                            }

                            if let Some(content) = key_content {
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

                        let result = interactor.create_user(register);
                        match result {
                            Ok(_) => println!("User created successfully"),

                            Err(e) => {
                                if e.to_string().contains("already exists") {
                                    println!("User already exists, no update");
                                } else {
                                    anyhow::bail!("Failed to create user: {}", e);
                                }
                            }
                        }
                    }
                }

                // 3. Prepare target directories (admin) and chown to deploy_user
                interactor.cmd(&format!("sudo mkdir -p '/opt/{}'", app.name))?;
                interactor.cmd(&format!(
                    "sudo chown -R '{}:{}' '/opt/{}'",
                    app.deploy_user, app.deploy_user, app.name
                ))?;
                interactor.cmd(&format!("sudo mkdir -p '/etc/crane/{}'", app.name))?;
                interactor.cmd(&format!(
                    "sudo chown -R '{}:{}' '/etc/crane/{}'",
                    app.deploy_user, app.deploy_user, app.name
                ))?;

                let deploy_private_key = find_private_key_for_user(&app.deploy_user, &config);
                let deploy_private_key = if deploy_private_key.is_empty() {
                    get_any_private_key(&config)
                } else {
                    deploy_private_key
                };
                // Create deploy SSH session (reuse distro detected via admin)
                let deploy_ssh = SSHSession::new(
                    node.host.clone(),
                    app.deploy_user.clone(),
                    deploy_private_key,
                    Some(node.port),
                );
                let deploy_interactor =
                    crate::server_interactor::get_interactor_for_distro(deploy_ssh, &node_distro)?;

                let release_dir = format!("/opt/{}/releases/{}", app.name, datetime);
                deploy_interactor.cmd(&format!("mkdir -p '{}'", release_dir))?;

                let remote_zip_path = format!("{}/deploy.zip", release_dir);
                deploy_interactor.upload(zip_path.to_str().unwrap(), &remote_zip_path)?;

                // Extract zip on server
                deploy_interactor.cmd(&format!(
                    "unzip -o '{}' -d '{}'",
                    remote_zip_path, release_dir
                ))?;
                println!("extracted zip to {}", release_dir);
                // Remove the remote zip file to clean up
                deploy_interactor.cmd(&format!("rm -f '{}'", remote_zip_path))?;

                // 4. Rolling deploy across vps instances
                let min_replicas = app
                    .min_replicas
                    .or_else(|| {
                        config
                            .monitor
                            .as_ref()
                            .and_then(|m| m.autoscale.as_ref())
                            .and_then(|a| a.min_replicas)
                    })
                    .unwrap_or(1);
                let max_replicas = app
                    .max_replicas
                    .or_else(|| {
                        config
                            .monitor
                            .as_ref()
                            .and_then(|m| m.autoscale.as_ref())
                            .and_then(|a| a.max_replicas)
                    })
                    .unwrap_or(4);

                let mut count = std::cmp::max(app.instances, min_replicas);
                count = std::cmp::min(count, max_replicas);

                let port_limit = app.port_end.unwrap_or(app.port_start + 100);
                let max_by_ports = if port_limit > app.port_start {
                    (port_limit - app.port_start) as u32
                } else {
                    0
                };
                count = std::cmp::min(count, max_by_ports);

                let port_end = app.port_start + count as u16;
                for port in app.port_start..port_end {
                    println!("Deploying instance of '{}' on port {}...", app.name, port);
                    let service_instance = format!("{}@{}", app.name, port);

                    // Stop service if running (admin)
                    let _ = interactor.stop_service(&service_instance);

                    // Update current symlink (deploy)
                    deploy_interactor.cmd(&format!(
                        "ln -sfn '{}' '/opt/{}/current'",
                        release_dir, app.name
                    ))?;

                    // Chmod the entrypoint to be executable
                    deploy_interactor.cmd(&format!(
                        "chmod +x '/opt/{}/current/{}'",
                        app.name,
                        app.entrypoint.trim_start_matches("./")
                    ))?;

                    // Write environment file (deploy)
                    let env_path = format!("/etc/crane/{}/.env", app.name);
                    deploy_interactor.create_file(&env_path, &env_content)?;
                    deploy_interactor.cmd(&format!("chmod 600 '{}'", env_path))?;

                    // Create systemd template unit (admin)
                    crate::systemd_unit::setup::setup_systemd_template(
                        &*interactor,
                        &app.name,
                        &app.deploy_user,
                        &app.entrypoint,
                    )?;

                    // Start service
                    interactor.cmd(&format!("sudo systemctl start '{}'", service_instance))?;

                    // Health check loop
                    let health_path = app.health_check_path.as_deref().unwrap_or("/health");
                    let timeout_secs = app.health_check_timeout.unwrap_or(30);
                    let interval_secs = app.health_check_interval.unwrap_or(2);

                    println!(
                        "\tPolling health check for {} on port {}...",
                        app.name, port
                    );
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
                            "\tHealth check failed for {} on port {} within {} seconds",
                            app.name,
                            port,
                            timeout_secs
                        );
                    }

                    println!("\tInstance {} on port {} is healthy!", app.name, port);
                }

                // 5. Write Traefik dynamic config
                let domain = app.domain.clone().unwrap_or_else(|| {
                    config
                        .domain
                        .as_ref()
                        .map(|d| d.name.clone())
                        .unwrap_or_else(|| "localhost".to_string())
                });
                let health_path = app.health_check_path.as_deref().unwrap_or("/health");
                crate::traefik_unit::setup::setup_traefik(
                    &*interactor,
                    &app.name,
                    &domain,
                    app.port_start,
                    port_end,
                    health_path,
                )?;

                // 5b. Update /etc/hosts on the VPS so apps can resolve each
                // other by service name (e.g. curl myapp2.localhost/curl?to=myapp).
                // Use domain's first label (e.g. "myapp2" from "myapp2.localhost") as hostname.
                let global_domain = config
                    .domain
                    .as_ref()
                    .map(|d| d.name.as_str())
                    .unwrap_or("localhost");
                let mut hosts_entries: Vec<(String, String)> = config
                    .app
                    .values()
                    .filter_map(|a| {
                        let dom = a.domain.as_deref().unwrap_or(&a.name);
                        // Strip the shared domain suffix to get just the service label
                        let hostname = if dom.ends_with(&format!(".{}", global_domain)) {
                            dom.trim_end_matches(&format!(".{}", global_domain))
                                .to_string()
                        } else {
                            dom.split('.').next().unwrap_or(dom).to_string()
                        };
                        Some((hostname, node.internal_ip.clone()))
                    })
                    .collect();
                // Dedup by hostname
                hosts_entries.sort_by(|a, b| a.0.cmp(&b.0));
                hosts_entries.dedup_by(|a, b| a.0 == b.0);
                println!("Updating /etc/hosts on node {}...", node.host);
                update_hosts(&*interactor, &hosts_entries)?;

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

            // Clean up local temporary zip file
            let _ = std::fs::remove_file(&zip_path);

            Ok(())
        });

        handles.push(handle);
    }

    for handle in handles {
        handle
            .join()
            .map_err(|e| anyhow::anyhow!("Thread panicked: {:?}", e))??;
    }

    println!("\n\nDEPLOY COMPLETE\n\n");

    Ok(())
}

/// Idempotently add/update `/etc/hosts` entries on the remote server.
/// For each (hostname, ip) pair: replace existing line if present, else append.
fn update_hosts(
    interactor: &dyn ServerInteractor,
    entries: &[(String, String)], // (hostname, ip)
) -> anyhow::Result<()> {
    for (hostname, ip) in entries {
        println!("  /etc/hosts: {} -> {}", hostname, ip);
        // Remove old entry for this hostname, then append the new one.
        // We use a temp file approach to avoid sed -i portability issues.
        let cmd = format!(
            r#"sudo sh -c 'grep -v " {hostname}" /etc/hosts > /tmp/hosts.tmp && echo "{ip} {hostname}" >> /tmp/hosts.tmp && cp /tmp/hosts.tmp /etc/hosts && rm /tmp/hosts.tmp'"#,
            hostname = hostname,
            ip = ip,
        );
        interactor.cmd(&cmd)?;
    }
    Ok(())
}
