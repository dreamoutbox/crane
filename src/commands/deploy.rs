use crate::config;
use crate::deployer::helper::{deploy_update_etc_hosts, deploy_zip_app};
use crate::deployer::users::deploy_setup_users;
use crate::helper::keys::find_private_key_for_user;
use crate::postgres_unit::helper::get_postgres_configs;
use crate::postgres_unit::setup::postgres_setup_wrapper;
use crate::server_interactor::get_server_interactor;
use crate::ssh::SSHSession;
use std::path::Path;

/// deploy app commands
pub fn run(
    config: &crate::config::Config,
    config_path: &Path,
    no_dns_update: bool,
) -> anyhow::Result<()> {
    let now = std::time::Instant::now();

    println!("Loading configuration from {:?}\n", config_path);

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
            "Installing dependencies on {}@{} (port: {}) {:?}",
            node.user, node.name, node.port, all_deps
        );
        let private_key = find_private_key_for_user(&node.user, &config)?;

        let ssh = SSHSession::new(
            node.host.clone(),
            node.user.clone(),
            private_key,
            Some(node.port),
        );

        //install required dependencies
        let interactor = get_server_interactor(ssh)?;
        interactor.install_dependencies(all_deps.clone())?;

        // install haproxy
        crate::haproxy_unit::haproxy::install_haproxy(&*interactor)?;
    }

    // Install postgres database cluster if enabled
    let pg_enabled = config
        .db
        .as_ref()
        .and_then(|db| db.postgres.as_ref())
        .map(|pg| pg.enabled)
        .unwrap_or(false);

    if pg_enabled {
        postgres_setup_wrapper(&config, &dot_env, &app_nodes)?;
    }

    // let mut handles = vec![];

    // Loop Apps in Config and deploy in parallel using threads
    for (_app_id, app) in config.app.clone() {
        let config_dir = config_dir.to_path_buf();
        let dot_env = dot_env.clone();
        let config = config.clone();
        let datetime = datetime.clone();

        // let handle = std::thread::spawn(move || -> anyhow::Result<()> , {});

        {
            // println!(
            //     "\nStarting deployment for app '{}' (ID: {})...",
            //     app.name, app_id
            // );
            println!("");

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
            let dir_to_deploy = deploy_dir_candidate.canonicalize()?;

            // zip app directory
            let zip_path = deploy_zip_app(&app, &datetime, dir_to_deploy)?;

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

            if let Some(ref app_db_deps) = app.database {
                let (db_configs, user_configs) = get_postgres_configs(&config);
                for db_dep in app_db_deps {
                    let user_name = &db_dep.user;
                    let user_pass = user_configs
                        .iter()
                        .find(|u| &u.user == user_name)
                        .and_then(|u| u.password.clone())
                        .unwrap_or_default();

                    let db_name = db_configs
                        .iter()
                        .find(|d| d.name == db_dep.databases || d.name == db_dep.databases)
                        .map(|d| d.name.as_str())
                        .unwrap_or(&db_dep.databases);

                    let db_env_key = db_name
                        .to_uppercase()
                        .replace(|c: char| !c.is_alphanumeric(), "_");

                    let build_uri = |port: u16| {
                        if !user_pass.is_empty() {
                            format!(
                                "postgresql://{}:{}@127.0.0.1:{}/{}",
                                user_name, user_pass, port, db_name
                            )
                        } else {
                            format!("postgresql://{}@127.0.0.1:{}/{}", user_name, port, db_name)
                        }
                    };

                    let leader_uri = build_uri(5000);
                    let follower_uri = build_uri(5001);

                    merged_env.insert(
                        format!("POSTGRES_{}_LEADER", db_env_key),
                        leader_uri.clone(),
                    );
                    merged_env.insert(format!("POSTGRES_{}_URI", db_env_key), leader_uri);
                    merged_env.insert(format!("POSTGRES_{}_FOLLOWER", db_env_key), follower_uri);
                }
            }

            let mut env_content = String::new();
            for (k, v) in &merged_env {
                env_content.push_str(&format!("{}={}\n", k, v));
            }

            for node in &app_nodes {
                println!(
                    "\n[{}] Deploying to node {}: {}@{} (port: {})",
                    app.name, node.name, node.user, node.host, node.port
                );

                let private_key = find_private_key_for_user(&node.user, &config)?;
                let ssh = SSHSession::new(
                    node.host.clone(),
                    node.user.clone(),
                    private_key,
                    Some(node.port),
                );
                let node_interactor = get_server_interactor(ssh)?;

                // 1. Setup user if specified
                deploy_setup_users(&app, &config, &*node_interactor)?;

                // 3. Prepare target directories (admin) and chown to deploy_user
                let app_dir = format!("/app/{}", app.name);
                let app_config_dir = format!("/app_config/{}", app.name);

                node_interactor.cmd(&format!("sudo mkdir -p '{}'", app_dir))?;
                node_interactor.cmd(&format!(
                    "sudo chown -R '{}:{}' '{}'",
                    app.deploy_user, app.deploy_user, app_dir
                ))?;
                node_interactor.cmd(&format!("sudo mkdir -p '{}'", app_config_dir))?;
                node_interactor.cmd(&format!(
                    "sudo chown -R '{}:{}' '{}'",
                    app.deploy_user, app.deploy_user, app_config_dir
                ))?;

                // Upload zip to /tmp to avoid permission issues
                let temp_zip_path = format!("/tmp/crane-deploy-{}-{}.zip", app.name, datetime);
                node_interactor.upload(zip_path.to_str().unwrap(), &temp_zip_path)?;

                // Extract zip on server using sudo
                node_interactor.cmd(&format!(
                    "sudo unzip -o '{}' -d '{}'",
                    temp_zip_path, app_dir
                ))?;
                // Ensure correct ownership of extracted files
                node_interactor.cmd(&format!(
                    "sudo chown -R '{}:{}' '{}'",
                    app.deploy_user, app.deploy_user, app_dir
                ))?;
                // Chmod the entrypoint to be executable
                node_interactor.cmd(&format!(
                    "sudo chmod +x '{}/{}'",
                    app_dir,
                    app.entrypoint.trim_start_matches("./")
                ))?;
                // Remove the remote temporary zip file
                node_interactor.cmd(&format!("rm -f '{}'", temp_zip_path))?;
                // println!("\tExtracted zip to {}\n", app_dir);

                // Run pre-deploy script if configured
                if let Some(ref pre_script) = app.pre_deploy_script {
                    let clean_script_path = pre_script.trim_start_matches("./");
                    let script_full_path = format!("{}/{}", app_dir, clean_script_path);
                    println!(
                        "\t[{}] Running pre-deploy script '{}'...",
                        app.name, clean_script_path
                    );

                    node_interactor.cmd(&format!("sudo chmod +x '{}'", script_full_path))?;
                    let out = node_interactor.cmd(&format!(
                        "sudo -u '{}' '{}'",
                        app.deploy_user, script_full_path
                    ))?;

                    if out.exit_code != 0 {
                        anyhow::bail!(
                            "\t[{}] pre-deploy script '{}' failed (exit code {}): {}",
                            app.name,
                            clean_script_path,
                            out.exit_code,
                            out.stderr
                        );
                    }
                }

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
                    println!("\t[{}] Deploying instance on port {} ...", app.name, port);
                    let service_instance = format!("{}@{}", app.name, port);

                    // Stop service if running (admin)
                    let _ = node_interactor.stop_service(&service_instance);

                    // No symlink update needed for direct /app deployment

                    // Write env file directly
                    let env_path = format!("{}/.env", app_config_dir);
                    node_interactor.create_file(&env_path, &env_content)?;
                    node_interactor.cmd(&format!(
                        "sudo chown '{}:{}' '{}'",
                        app.deploy_user, app.deploy_user, env_path
                    ))?;
                    node_interactor.cmd(&format!("sudo chmod 600 '{}'", env_path))?;

                    // Create systemd template unit (admin)
                    crate::systemd_unit::setup::setup_systemd_template(
                        &*node_interactor,
                        &app.name,
                        &app.deploy_user,
                        &app.entrypoint,
                    )?;

                    // Start service
                    node_interactor.start_service(&service_instance)?;

                    // Health check loop
                    let health_path = app.health_check_path.as_deref().unwrap_or("/health");
                    let timeout_secs = app.health_check_timeout.unwrap_or(30);

                    println!("\t[{}] polling health check on port {}...", app.name, port);
                    let mut healthy = false;
                    let start_time = std::time::Instant::now();

                    while start_time.elapsed().as_secs() < timeout_secs {
                        let curl_cmd = format!(
                            "curl -s -o /dev/null -w \"%{{http_code}}\" http://127.0.0.1:{}{}",
                            port, health_path
                        );

                        if let Ok(code) = node_interactor.cmd(&curl_cmd) {
                            if code.stdout.trim() == "200" {
                                healthy = true;
                                break;
                            }
                        }

                        std::thread::sleep(std::time::Duration::from_millis(800));
                    }

                    if !healthy {
                        anyhow::bail!(
                            "\t{} health check failed on port {} within {} seconds",
                            app.name,
                            port,
                            timeout_secs
                        );
                    }

                    println!("\t[{}] instance on port {} is healthy!", app.name, port);
                }

                // 5. Write unified HAProxy config
                crate::haproxy_unit::haproxy::setup_haproxy_unified(
                    &*node_interactor,
                    &config,
                    node,
                    Some(&app.name),
                    Some(port_end),
                )?;

                // 5b. Update /etc/hosts on the VPS so apps can resolve each other by service name
                // e.g. curl myapp2/curl?to=myapp
                // Use domain's first label (e.g. "myapp2" from "myapp2.localhost") as hostname.
                let global_domain = config
                    .domain
                    .as_ref()
                    .map(|d| d.domain_name.as_str())
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
                        Some((hostname, "127.0.0.1".to_string()))
                    })
                    .collect();

                // Dedup by hostnamesetup_traefik
                hosts_entries.sort_by(|a, b| a.0.cmp(&b.0));
                hosts_entries.dedup_by(|a, b| a.0 == b.0);

                println!("\tUpdating /etc/hosts on node {}...", node.name);
                deploy_update_etc_hosts(&*node_interactor, &hosts_entries)?;

                // No pruning needed for direct /app deployment
            }

            // Clean up local temporary zip file
            let _ = std::fs::remove_file(&zip_path);

            // Ok(())
        }

        // handles.push(handle);
    }

    // for handle in handles {
    //     handle
    //         .join()
    //         .map_err(|e| anyhow::anyhow!("Thread panicked: {:?}", e))??;
    // }

    let deploy_elapse = now.elapsed();
    println!("\nDEPLOY COMPLETE ({} secs)\n", deploy_elapse.as_secs());

    if !no_dns_update {
        if let Err(e) = crate::cloudflare_unit::setup::update_dns_blocking(&config, None, true) {
            eprintln!("Failed to update DNS records: {}", e);
        }
    }

    Ok(())
}
