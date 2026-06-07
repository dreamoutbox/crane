use crate::config::get_postgres_dbs_and_users_config;
use crate::deployer::helper::{deploy_update_etc_hosts, deploy_zip_app};
use crate::deployer::users::deploy_setup_app_users;
use crate::helper::config::config_get_nodes;
use crate::postgres_unit::setup::postgres_setup_wrapper;
use crate::server_interactor::get_server_interactor;
use std::path::Path;

/// deploy app commands
pub async fn run_deploy_command(
    config: &crate::config::Config,
    config_path: &Path,
    no_dns_update: bool,
) -> anyhow::Result<()> {
    let now = std::time::Instant::now();

    println!("Loading configuration from {:?}\n", config_path);
    // dbg!(&config);

    let config_dir = config_path.parent().unwrap_or(Path::new("."));

    // Get datetime for release
    let datetime_output = std::process::Command::new("date")
        .arg("+%Y%m%d_%H%M%S")
        .output()?;
    let datetime = String::from_utf8_lossy(&datetime_output.stdout)
        .trim()
        .to_string();
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

    let app_nodes = config_get_nodes(&config, "app");
    let pg_nodes = config_get_nodes(&config, "postgres");

    let mut handles = vec![];
    for node in &pg_nodes {
        let node = node.clone();
        let all_deps = all_deps.clone();

        let handle = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            println!(
                "Installing dependencies on {}@{} (port: {}) {:?}",
                node.user, node.name, node.ssh_port, all_deps
            );

            let interactor = get_server_interactor(&node.name)?;

            //install required dependencies
            interactor.install_dependencies(all_deps.clone())?;

            // install haproxy
            crate::haproxy_unit::haproxy::install_haproxy(&*interactor)?;

            Ok(())
        });

        handles.push(handle);
    }
    for handle in handles {
        handle.await??;
    }

    // Install postgres database cluster if enabled
    let pg_enabled = config
        .db
        .as_ref()
        .and_then(|db| db.postgres.as_ref())
        .map(|pg| pg.enabled)
        .unwrap_or(false);
    if pg_enabled {
        postgres_setup_wrapper(&config, &pg_nodes).await?;
    }

    let mut deploy_handles = vec![];

    // Loop Apps in Config and deploy in parallel using tokio spawn_blocking
    for (app_id, app) in config.app.clone() {
        let config_dir = config_dir.to_path_buf();
        let config = config.clone();
        let datetime = datetime.clone();
        let app_nodes = app_nodes.clone();
        let pg_nodes = pg_nodes.clone();

        let handle = tokio::task::spawn_blocking(move || -> (Vec<String>, anyhow::Result<()>) {
            let mut logs = Vec::new();
            let res = inner_deploy_single_app(
                &app_id,
                &app,
                &config,
                &config_dir,
                &datetime,
                &app_nodes,
                &pg_nodes,
                &mut logs,
            );
            (logs, res)
        });

        deploy_handles.push(handle);
    }

    let mut has_error = None;
    for handle in deploy_handles {
        match handle.await {
            Ok((logs, res)) => {
                for log_line in logs {
                    println!("{}", log_line);
                }

                if let Err(e) = res {
                    if has_error.is_none() {
                        has_error = Some(e);
                    }
                }
            }

            Err(e) => {
                if has_error.is_none() {
                    has_error = Some(anyhow::anyhow!("Task panicked: {:?}", e));
                }
            }
        }
    }

    if let Some(err) = has_error {
        return Err(err);
    }

    // Reload HAProxy once at the end on all app nodes
    crate::haproxy_unit::haproxy::reload_haproxy_on_each_nodes_wrapper(&app_nodes).await?;

    // Manage firewall
    println!("\nConfiguring firewalls on all nodes...");
    for node in &config.nodes {
        println!("\tSetting up firewall on {}...", node.name);
        let interactor = get_server_interactor(&node.name)?;
        crate::firewall::setup::setup_firewall(&*interactor, config)?;
    }

    let deploy_elapse = now.elapsed();
    println!("\nDEPLOY COMPLETE ({} secs)\n", deploy_elapse.as_secs());

    if !no_dns_update {
        if let Err(e) = crate::cloudflare_unit::setup::update_dns(&config, None).await {
            eprintln!("Failed to update DNS records: {}", e);
        }
    }

    Ok(())
}

fn inner_deploy_single_app(
    app_id: &str,
    app: &crate::config::AppConfig,
    config: &crate::config::Config,
    config_dir: &std::path::Path,
    datetime: &String,
    app_nodes: &[crate::config::NodeConfig],
    pg_nodes: &[crate::config::NodeConfig],
    logs: &mut Vec<String>,
) -> anyhow::Result<()> {
    macro_rules! log {
        ($($arg:tt)*) => {
            logs.push(format!($($arg)*));
        };
    }

    log!(
        "\nStarting deployment for app '{}' (ID: {})...",
        app.name,
        app_id
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
    let dir_to_deploy = deploy_dir_candidate.canonicalize()?;

    // zip app directory
    let zip_path = deploy_zip_app(app, datetime, dir_to_deploy)?;

    // Merge environment
    let mut app_env = std::collections::HashMap::new();
    if let Some(ref env_map) = app.env {
        for (k, v) in env_map {
            app_env.insert(k.clone(), v.clone());
        }
    }

    if let Some(ref app_db_deps) = app.database {
        let (db_configs, user_configs) = get_postgres_dbs_and_users_config(config);
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

            let follower_uri = if pg_nodes.len() > 1 {
                build_uri(5001)
            } else {
                leader_uri.clone()
            };

            app_env.insert(
                format!("POSTGRES_{}_LEADER", db_env_key),
                leader_uri.clone(),
            );
            app_env.insert(format!("POSTGRES_{}_URI", db_env_key), leader_uri);
            app_env.insert(format!("POSTGRES_{}_FOLLOWER", db_env_key), follower_uri);
        }
    }

    let mut env_content = String::new();
    for (k, v) in &app_env {
        env_content.push_str(&format!("{}={}\n", k, v));
    }

    for node in app_nodes {
        log!(
            "\n[{}] Deploying to node {}: {}@{} (port: {})",
            app.name,
            node.name,
            node.user,
            node.ssh_ip,
            node.ssh_port
        );

        let node_interactor = get_server_interactor(&node.name)?;

        // 1. Setup user if specified
        deploy_setup_app_users(app, config, &*node_interactor)?;

        // 3. Prepare target directories (admin) and chown to deploy_user
        let app_dir = format!("/app/{}", app.name);
        let app_config_dir = format!("/app_config/{}", app.name);

        // create app dir
        node_interactor.mkdir(&app_dir)?;
        node_interactor.chown(&app_dir, &app.deploy_user, &app.deploy_user)?;

        // create app config dir
        node_interactor.mkdir(&app_config_dir)?;
        node_interactor.chown(&app_config_dir, &app.deploy_user, &app.deploy_user)?;

        // Upload zip to /tmp to avoid permission issues
        let temp_zip_path = format!("/tmp/crane-deploy-{}-{}.zip", app.name, datetime);
        node_interactor.upload(zip_path.to_str().unwrap(), &temp_zip_path)?;

        // Extract zip on server using sudo
        node_interactor.unzip(&temp_zip_path, &app_dir)?;
        // Ensure correct ownership of extracted files
        node_interactor.chown(&app_dir, &app.deploy_user, &app.deploy_user)?;
        // Chmod the entrypoint to be executable
        node_interactor.cmd(&format!(
            "sudo chmod +x '{}/{}'",
            app_dir,
            app.entrypoint.trim_start_matches("./")
        ))?;
        // Remove the remote temporary zip file
        node_interactor.cmd(&format!("rm -f '{}'", temp_zip_path))?;

        // Run pre-deploy script if configured
        if let Some(ref pre_script) = app.pre_deploy_script {
            let clean_script_path = pre_script.trim_start_matches("./");
            let script_full_path = format!("{}/{}", app_dir, clean_script_path);
            log!(
                "\t[{}] Running pre-deploy script '{}'...",
                app.name,
                clean_script_path
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
            // .or_else(|| {
            //     config
            //         .monitor
            //         .as_ref()
            //         .and_then(|m| m.autoscale.as_ref())
            //         .and_then(|a| a.min_replicas)
            // })
            .unwrap_or(1);

        let max_replicas = app
            .max_replicas
            // .or_else(|| {
            //     config
            //         .monitor
            //         .as_ref()
            //         .and_then(|m| m.autoscale.as_ref())
            //         .and_then(|a| a.max_replicas)
            // })
            .unwrap_or(1);

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
            log!("\t[{}] Deploying instance on port {} ...", app.name, port);
            let service_instance = format!("{}@{}", app.name, port);

            let mut env_content_for_app = env_content.clone();
            // add env PORT for this app
            env_content_for_app.push_str(&format!("PORT={}\n", port));

            // Stop service if running (admin)
            let _ = node_interactor.stop_service(&service_instance);

            //Create app config directory for this instance
            let this_app_config_dir = format!("{}/{}", app_config_dir, port);
            node_interactor.mkdir(&this_app_config_dir)?;
            // Write env file
            let env_path = format!("{}/.env", this_app_config_dir);
            node_interactor.create_file(&env_path, &env_content_for_app)?;
            //Fix perrmissions
            node_interactor.chown(&this_app_config_dir, &app.deploy_user, &app.deploy_user)?;
            node_interactor.chmod(&this_app_config_dir, "600")?;

            // Create systemd template unit (admin)
            let env_template_path = format!("/app_config/{}/%i/.env", app.name);
            crate::systemd_unit::setup::setup_systemd_template(
                &*node_interactor,
                &app.name,
                &app.deploy_user,
                &app.entrypoint,
                &env_template_path,
            )?;

            // Enable service instance
            node_interactor.enable_service(&service_instance)?;

            // Start service
            node_interactor.start_service(&service_instance)?;

            // Health check loop
            let health_path = app.health_check_path.as_deref().unwrap_or("/health");
            let timeout_secs = app.health_check_timeout.unwrap_or(30);

            log!("\t[{}] polling health check on port {}...", app.name, port);
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

            log!("\t[{}] instance on port {} is healthy!", app.name, port);
        }

        // 5. Write unified HAProxy config
        crate::haproxy_unit::haproxy::setup_haproxy_unified(
            &*node_interactor,
            config,
            node,
            Some(&app.name),
            Some(port_end),
        )?;

        // 5b. Update /etc/hosts on the VPS so apps can resolve each other by service name
        let global_domain = config
            .domain
            .as_ref()
            .map(|d| d.domain_name.as_str())
            .unwrap_or("localhost");

        let mut etc_hosts: Vec<(String, String)> = config
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
        // Dedup by hostname
        etc_hosts.sort_by(|a, b| a.0.cmp(&b.0));
        etc_hosts.dedup_by(|a, b| a.0 == b.0);

        log!("\tUpdating /etc/hosts on node {}...", node.name);
        deploy_update_etc_hosts(&*node_interactor, &etc_hosts)?;
    }

    // Clean up local temporary zip file
    let _ = std::fs::remove_file(&zip_path);

    Ok(())
}
