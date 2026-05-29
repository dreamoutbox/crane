use crate::config;
use crate::helper::keys::find_private_key_for_user;
use crate::ssh::SSHSession;
use std::io::{BufRead, BufReader};

pub fn run(
    config: crate::config::Config,
    app_target: &str,
    lines: u32,
    since: Option<&str>,
    until: Option<&str>,
    show_timestamps: bool,
    follow: bool,
    no_app_instance_id: bool,
) -> anyhow::Result<()> {
    // 1. Parse app target (e.g. "myapp" or "myapp@1")
    let (app_name, target_instance_id) = if let Some((name, id_str)) = app_target.split_once('@') {
        let id = id_str
            .parse::<usize>()
            .map_err(|_| anyhow::anyhow!("Invalid instance ID: '{}'", id_str))?;
        (name.to_string(), Some(id))
    } else {
        (app_target.to_string(), None)
    };

    // 3. Find the app config
    let (_app_id, app) = config
        .app
        .iter()
        .find(|(id, a)| id.as_str() == app_name || a.name == app_name)
        .ok_or_else(|| anyhow::anyhow!("App '{}' not found in configuration", app_name))?;

    // 4. Find all app nodes
    let app_nodes: Vec<_> = config
        .nodes
        .iter()
        .filter(|n| n.roles.contains(&"app".to_string()))
        .cloned()
        .collect();

    if app_nodes.is_empty() {
        anyhow::bail!("No nodes with the 'app' role found in configuration");
    }

    // 5. Compute global instance IDs
    let instances_per_node = app.instances as usize;
    let total_instances = app_nodes.len() * instances_per_node;

    if let Some(id) = target_instance_id {
        if id < 1 || id > total_instances {
            anyhow::bail!(
                "Instance ID {} is invalid. Total instances: {}",
                id,
                total_instances
            );
        }
    }

    // 6. Build target instances mapping
    struct TargetedInstance {
        instance_id: usize,
        node: config::NodeConfig,
        port: u16,
    }

    let mut targets = Vec::new();
    for (node_idx, node) in app_nodes.iter().enumerate() {
        for port_idx in 0..instances_per_node {
            let instance_id = node_idx * instances_per_node + port_idx + 1;
            let port = app.port_start + port_idx as u16;

            if target_instance_id.is_none() || target_instance_id == Some(instance_id) {
                targets.push(TargetedInstance {
                    instance_id,
                    node: node.clone(),
                    port,
                });
            }
        }
    }

    // Helper to build journalctl command string
    let build_cmd = |port: u16| -> String {
        let mut cmd = format!(
            "sudo journalctl -u {}@{}.service -n {}",
            app.name, port, lines
        );
        if let Some(since_val) = since {
            cmd.push_str(&format!(" --since '{}'", since_val.replace('\'', "'\\''")));
        }
        if let Some(until_val) = until {
            cmd.push_str(&format!(" --until '{}'", until_val.replace('\'', "'\\''")));
        }
        if show_timestamps {
            cmd.push_str(" --output=short-iso");
        } else {
            cmd.push_str(" --output=cat");
        }
        if follow {
            cmd.push_str(" -f");
        }
        cmd
    };

    if follow {
        // Stream in parallel using raw SSH Session child processes
        let mut handles = vec![];

        for target in targets {
            let cmd = build_cmd(target.port);

            let private_key = find_private_key_for_user(&target.node.user, &config)?;
            let ssh = SSHSession::new(
                target.node.host.clone(),
                target.node.user.clone(),
                private_key,
                Some(target.node.port),
            );

            let app_name = app.name.clone();
            let instance_id = target.instance_id;

            let handle = std::thread::spawn(move || -> anyhow::Result<()> {
                let mut child = ssh.spawn_cmd(&cmd)?;
                let stdout = child.stdout.take().ok_or_else(|| {
                    anyhow::anyhow!("Failed to open stdout for instance {}", instance_id)
                })?;

                let reader = BufReader::new(stdout);
                for line in reader.lines() {
                    let line = line?;
                    if no_app_instance_id {
                        println!("{}", line);
                    } else {
                        println!("[{}@{}] {}", app_name, instance_id, line);
                    }
                }

                Ok(())
            });

            handles.push(handle);
        }

        for handle in handles {
            let _ = handle.join();
        }
    } else {
        // Query in parallel using interactors, print grouped/ordered by instance ID
        let mut handles = vec![];

        for target in targets {
            let cmd = build_cmd(target.port);
            let private_key = find_private_key_for_user(&target.node.user, &config)?;
            let ssh = SSHSession::new(
                target.node.host.clone(),
                target.node.user.clone(),
                private_key,
                Some(target.node.port),
            );
            let instance_id = target.instance_id;

            let handle = std::thread::spawn(move || -> anyhow::Result<Vec<String>> {
                let interactor = crate::server_interactor::get_server_interactor(ssh)?;

                let output = interactor.cmd(&cmd)?;
                if output.exit_code != 0 {
                    anyhow::bail!(
                        "Command failed (exit code {}): {}",
                        output.exit_code,
                        output.stderr
                    );
                }

                let lines: Vec<String> = output.stdout.lines().map(|s| s.to_string()).collect();
                Ok(lines)
            });

            handles.push((instance_id, handle));
        }

        for (instance_id, handle) in handles {
            match handle.join() {
                Ok(Ok(lines)) => {
                    for line in lines {
                        if no_app_instance_id {
                            println!("{}", line);
                        } else {
                            println!("[{}@{}] {}", app.name, instance_id, line);
                        }
                    }
                }

                Ok(Err(e)) => {
                    eprintln!("Error fetching logs for instance {}: {}", instance_id, e);
                }

                Err(_) => {
                    eprintln!(
                        "Thread panicked while fetching logs for instance {}",
                        instance_id
                    );
                }
            }
        }
    }

    Ok(())
}
