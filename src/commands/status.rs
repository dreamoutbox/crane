use crate::helper::config::config_get_nodes;
use crate::server_interactor::get_server_interactor;
use std::collections::{BTreeSet, HashMap};

#[derive(Clone)]
pub struct NetworkInterfaceStatus {
    pub name: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_rate_kbps: f64,
    pub tx_rate_kbps: f64,
}

pub struct NodeResourceStatus {
    pub cpu_usage_pct: Option<f64>,
    pub mem_total_mb: Option<u64>,
    pub mem_used_mb: Option<u64>,
    pub mem_use_pct: Option<f64>,
    pub disk_info: Option<(String, String, String)>, // (Size, Used, Use%)
    pub network_interfaces: Vec<NetworkInterfaceStatus>,
}

pub struct NodeAppInstancesStatus {
    // Maps (app_name, port) -> (systemd_active, systemd_substate, http_status)
    pub instances: HashMap<(String, u16), (bool, String, Option<u16>)>,
}

pub struct NodeStatusResult {
    pub node_name: String,
    pub node_public_ip: String,
    pub node_internal_ip: String,
    pub node_ssh_port: u16,
    pub status: Result<(NodeResourceStatus, NodeAppInstancesStatus), String>,
}

pub fn run_status_command(
    config: &crate::config::Config,
    app_name: Option<&str>,
) -> anyhow::Result<()> {
    // Find requested apps to query
    let apps_to_query: Vec<&crate::config::AppConfig> = if let Some(name) = app_name {
        let (_, app) = config
            .app
            .iter()
            .find(|(id, a)| *id == name || a.name == name)
            .ok_or_else(|| anyhow::anyhow!("App '{}' not found in configuration", name))?;
        vec![app]
    } else {
        let mut apps: Vec<&crate::config::AppConfig> = config.app.values().collect();
        apps.sort_by_key(|a| &a.name);
        apps
    };

    // Build the combined ports to check across the nodes
    let mut combined_ports = BTreeSet::new();
    for app in &apps_to_query {
        let port_end = app.port_start + app.instances as u16;
        for port in app.port_start..port_end {
            combined_ports.insert((
                port,
                app.health_check_path
                    .as_deref()
                    .unwrap_or("/health")
                    .to_string(),
            ));
        }
    }

    let mut handles = vec![];
    for (idx, node) in config.nodes.iter().enumerate() {
        let node_clone = node.clone();
        let combined_ports_clone = combined_ports.clone();
        let apps_for_instances_clone: Vec<crate::config::AppConfig> =
            apps_to_query.iter().map(|a| (*a).clone()).collect();

        let handle = std::thread::spawn(move || -> (usize, NodeStatusResult) {
            let has_app_role = node_clone.roles.contains(&"app".to_string());
            let result = (|| -> anyhow::Result<(NodeResourceStatus, NodeAppInstancesStatus)> {
                let interactor = get_server_interactor(&node_clone.name)?;

                let mut systemd_units = Vec::new();
                let mut http_statuses = HashMap::new();

                if has_app_role {
                    // Query service instances from systemd to dynamically discover active ports
                    let systemd_cmd = "systemctl list-units --type=service --all \"*@*.service\" --no-legend --no-pager";
                    let systemd_output = interactor.cmd(systemd_cmd)?;

                    for line in systemd_output.stdout.lines() {
                        let line = line.trim();
                        if line.is_empty() {
                            continue;
                        }
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 4 {
                            let unit_name = parts[0];
                            if let Some(at_idx) = unit_name.find('@') {
                                if let Some(dot_idx) = unit_name.find(".service") {
                                    if at_idx < dot_idx {
                                        let app_prefix = &unit_name[..at_idx];
                                        let port_str = &unit_name[at_idx + 1..dot_idx];
                                        if let Ok(port) = port_str.parse::<u16>() {
                                            let active_state = parts[2].to_string();
                                            let sub_state = parts[3].to_string();

                                            systemd_units.push((
                                                app_prefix.to_string(),
                                                port,
                                                active_state,
                                                sub_state,
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Determine combined curls
                    let mut curl_cmds = Vec::new();
                    for (port, path) in &combined_ports_clone {
                        curl_cmds.push(format!(
                            "curl -s -o /dev/null -w \"PORT:{}:STATUS:%{{http_code}}\\n\" http://127.0.0.1:{}{} || echo \"PORT:{}:STATUS:000\"",
                            port, port, path, port
                        ));
                    }

                    let combined_curl_cmd = if curl_cmds.is_empty() {
                        "echo \"\"".to_string()
                    } else {
                        curl_cmds.join("; ")
                    };

                    // Combined system resource metrics query (one-shot SSH command)
                    let remote_cmd = format!(
                        "cat /proc/meminfo && \
                         echo \"===DF===\" && \
                         df -h / && \
                         echo \"===METRICS===\" && \
                         cat /proc/stat && \
                         echo \"===NET===\" && \
                         cat /proc/net/dev && \
                         echo \"===SPLIT===\" && \
                         sleep 0.2 && \
                         cat /proc/stat && \
                         echo \"===NET===\" && \
                         cat /proc/net/dev && \
                         echo \"===CURLS===\" && \
                         {}",
                        combined_curl_cmd
                    );

                    let output_raw = interactor.cmd(&remote_cmd)?;
                    if output_raw.exit_code != 0 {
                        anyhow::bail!("Failed to query system metrics: {}", output_raw.stderr);
                    }

                    let output = output_raw.stdout;

                    // Split metrics and curl results
                    let parts: Vec<&str> = output.split("===CURLS===").collect();
                    if parts.len() < 2 {
                        anyhow::bail!("Invalid command output format from node");
                    }
                    let metrics_part = parts[0];
                    let curls_part = parts[1];

                    // Parse Metrics
                    let (resource_status, _) = parse_metrics(metrics_part)?;

                    // Parse Curls Status
                    for line in curls_part.lines() {
                        let line = line.trim();
                        if line.starts_with("PORT:") {
                            let parts: Vec<&str> = line.split(':').collect();
                            if parts.len() >= 4 {
                                if let Ok(port) = parts[1].parse::<u16>() {
                                    if let Ok(status) = parts[3].parse::<u16>() {
                                        http_statuses.insert(port, status);
                                    }
                                }
                            }
                        }
                    }

                    // Map instances status
                    let mut instances = HashMap::new();
                    for app in &apps_for_instances_clone {
                        let port_end = app.port_start + app.instances as u16;
                        for port in app.port_start..port_end {
                            let systemd_state = systemd_units
                                .iter()
                                .find(|(prefix, p, _, _)| prefix == &app.name && *p == port);
                            let (sys_active, sys_sub) = match systemd_state {
                                Some((_, _, active_state, sub_state)) => {
                                    (active_state.clone(), sub_state.clone())
                                }
                                None => ("inactive".to_string(), "not-found".to_string()),
                            };
                            let http_status = http_statuses.get(&port).copied();
                            instances.insert(
                                (app.name.clone(), port),
                                (sys_active == "active", sys_sub, http_status),
                            );
                        }
                    }

                    Ok((resource_status, NodeAppInstancesStatus { instances }))
                } else {
                    // Node does not have app role, only query metrics
                    let remote_cmd = "cat /proc/meminfo && \
                                      echo \"===DF===\" && \
                                      df -h / && \
                                      echo \"===METRICS===\" && \
                                      cat /proc/stat && \
                                      echo \"===NET===\" && \
                                      cat /proc/net/dev && \
                                      echo \"===SPLIT===\" && \
                                      sleep 0.2 && \
                                      cat /proc/stat && \
                                      echo \"===NET===\" && \
                                      cat /proc/net/dev";

                    let output_raw = interactor.cmd(remote_cmd)?;
                    if output_raw.exit_code != 0 {
                        anyhow::bail!("Failed to query system metrics: {}", output_raw.stderr);
                    }

                    let (resource_status, _) = parse_metrics(&output_raw.stdout)?;
                    Ok((
                        resource_status,
                        NodeAppInstancesStatus {
                            instances: HashMap::new(),
                        },
                    ))
                }
            })();

            (
                idx,
                NodeStatusResult {
                    node_name: node_clone.name.clone(),
                    node_public_ip: node_clone.public_ip.clone(),
                    node_internal_ip: node_clone.internal_ip.clone(),
                    node_ssh_port: node_clone.ssh_port,
                    status: result.map_err(|e| e.to_string()),
                },
            )
        });
        handles.push(handle);
    }

    let mut results = Vec::new();
    for _ in 0..config.nodes.len() {
        results.push(None);
    }

    for handle in handles {
        if let Ok((idx, res)) = handle.join() {
            results[idx] = Some(res);
        }
    }

    let nodes_results: Vec<NodeStatusResult> = results.into_iter().flatten().collect();

    // Print Header
    println!("NODE:");
    println!();

    // Print Node Statuses
    for node_res in &nodes_results {
        println!("{}", node_res.node_name);
        println!("  Public IP: {}", node_res.node_public_ip);
        println!("  Internal IP: {}", node_res.node_internal_ip);
        println!("  SSH Port: {}", node_res.node_ssh_port);

        match &node_res.status {
            Err(err_str) => {
                println!("  Unreachable: {}", err_str);
            }
            Ok((resource, _)) => {
                if let (Some(cpu), Some(used_mem), Some(total_mem), Some(pct_mem)) = (
                    resource.cpu_usage_pct,
                    resource.mem_used_mb,
                    resource.mem_total_mb,
                    resource.mem_use_pct,
                ) {
                    println!("  CPU Usage:  {:.1}%", cpu);
                    println!(
                        "  Memory:     {} MB / {} MB ({:.1}%)",
                        used_mem, total_mem, pct_mem
                    );
                } else {
                    println!("  CPU Usage:  Unavailable");
                    println!("  Memory:     Unavailable");
                }

                if let Some((size, used, pct)) = &resource.disk_info {
                    println!("  Disk (/):   {} / {} ({})", used, size, pct);
                } else {
                    println!("  Disk (/):   Unavailable");
                }

                println!("  Network:");
                if resource.network_interfaces.is_empty() {
                    println!("    No active interfaces found.");
                } else {
                    let mut sorted_ifaces = resource.network_interfaces.clone();
                    sorted_ifaces.sort_by_key(|n| n.name.clone());
                    for net in &sorted_ifaces {
                        println!(
                            "    - {}: Rx: {} ({:.1} KB/s), Tx: {} ({:.1} KB/s)",
                            net.name,
                            format_bytes(net.rx_bytes),
                            net.rx_rate_kbps,
                            format_bytes(net.tx_bytes),
                            net.tx_rate_kbps
                        );
                    }
                }
            }
        }
        println!();
    }

    // Print APP Header
    println!("APP:");
    println!();

    // Print App Statuses
    for app in &apps_to_query {
        println!("{}:", app.name);

        // Determine external URL
        let external_url = if app.domain.is_some() {
            format!("https://{}", app.domain.as_ref().unwrap())
        } else if let Some(ref d) = config.domain {
            format!("https://{}", d.domain_name)
        } else {
            // Find first app node
            let app_nodes = config_get_nodes(config, "app");
            if !app_nodes.is_empty() {
                format!("http://{}", app_nodes[0].public_ip)
            } else {
                "http://localhost".to_string()
            }
        };

        let port_range_str = match app.port_end {
            Some(end) => format!("{}-{}", app.port_start, end),
            None => format!("{}", app.port_start),
        };

        // Determine overall status
        let mut total_instances = 0;
        let mut healthy_instances = 0;
        let mut has_any_node_offline = false;

        let app_nodes = config_get_nodes(config, "app");
        for node in &app_nodes {
            if let Some(node_res) = nodes_results.iter().find(|nr| nr.node_name == node.name) {
                match &node_res.status {
                    Err(_) => {
                        has_any_node_offline = true;
                        total_instances += app.instances as usize;
                    }
                    Ok((_, instances_status)) => {
                        let port_end = app.port_start + app.instances as u16;
                        for port in app.port_start..port_end {
                            total_instances += 1;
                            if let Some(&(sys_active, _, http_status)) =
                                instances_status.instances.get(&(app.name.clone(), port))
                            {
                                if sys_active && http_status == Some(200) {
                                    healthy_instances += 1;
                                }
                            }
                        }
                    }
                }
            }
        }

        let overall_status =
            if total_instances > 0 && healthy_instances == total_instances && !has_any_node_offline
            {
                "HEALTHY"
            } else if healthy_instances > 0 || (total_instances > 0 && !has_any_node_offline) {
                "DEGRADED"
            } else {
                "UNHEALTHY"
            };

        println!("  External URL:    {}", external_url);
        println!("  Port Range:      {}", port_range_str);
        println!("  Overall Status:  {}", overall_status);
        println!();

        // Print instances on each app node
        for (node_index, node) in app_nodes.iter().enumerate() {
            if let Some(node_res) = nodes_results.iter().find(|nr| nr.node_name == node.name) {
                match &node_res.status {
                    Err(_) => {
                        let port_end = app.port_start + app.instances as u16;
                        for (port_index, port) in (app.port_start..port_end).enumerate() {
                            let instance_id =
                                node_index * (app.instances as usize) + port_index + 1;
                            println!(
                                "  {}@{} ({}) (Port {}) Inactive Connection Failed",
                                app.name, instance_id, node.name, port
                            );
                        }
                    }
                    Ok((_, instances_status)) => {
                        let port_end = app.port_start + app.instances as u16;
                        for (port_index, port) in (app.port_start..port_end).enumerate() {
                            let instance_id =
                                node_index * (app.instances as usize) + port_index + 1;
                            if let Some(&(sys_active, ref sys_sub, http_status)) =
                                instances_status.instances.get(&(app.name.clone(), port))
                            {
                                let sys_status_str = if sys_active {
                                    "Active".to_string()
                                } else {
                                    format!("Inactive ({})", sys_sub)
                                };

                                let http_status_str = match http_status {
                                    Some(200) => "200 OK".to_string(),
                                    Some(code) if code > 0 => format!("{}", code),
                                    _ => "Connection Failed".to_string(),
                                };

                                println!(
                                    "  {}@{} ({}) (Port {}) {} {}",
                                    app.name,
                                    instance_id,
                                    node.name,
                                    port,
                                    sys_status_str,
                                    http_status_str
                                );
                            } else {
                                println!(
                                    "  {}@{} ({}) (Port {}) Inactive not-found Connection Failed",
                                    app.name, instance_id, node.name, port
                                );
                            }
                        }
                    }
                }
            }
        }
        println!();
    }

    Ok(())
}

fn parse_metrics(metrics_part: &str) -> anyhow::Result<(NodeResourceStatus, ())> {
    let metrics_split: Vec<&str> = metrics_part.split("===METRICS===").collect();
    if metrics_split.len() < 2 {
        anyhow::bail!("Invalid metrics section in output");
    }
    let mem_df = metrics_split[0];
    let cpu_net = metrics_split[1];

    let mem_df_split: Vec<&str> = mem_df.split("===DF===").collect();
    if mem_df_split.len() < 2 {
        anyhow::bail!("Invalid memory/disk section in output");
    }
    let mem_raw = mem_df_split[0];
    let df_raw = mem_df_split[1];

    // Memory Parse
    let mut mem_total_kb = None;
    let mut mem_available_kb = None;
    for line in mem_raw.lines() {
        if line.starts_with("MemTotal:") {
            mem_total_kb = line
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse::<u64>().ok());
        } else if line.starts_with("MemAvailable:") {
            mem_available_kb = line
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse::<u64>().ok());
        }
    }
    let (mem_used_mb, mem_total_mb, mem_use_pct) = match (mem_total_kb, mem_available_kb) {
        (Some(total), Some(avail)) => {
            let total_mb = total / 1024;
            let avail_mb = avail / 1024;
            let used_mb = total_mb.saturating_sub(avail_mb);
            let pct = if total_mb > 0 {
                (used_mb as f64 / total_mb as f64) * 100.0
            } else {
                0.0
            };
            (Some(used_mb), Some(total_mb), Some(pct))
        }
        _ => (None, None, None),
    };

    // Disk Parse
    let df_lines: Vec<&str> = df_raw.lines().filter(|l| !l.is_empty()).collect();
    let mut disk_info = None;
    if df_lines.len() >= 2 {
        let fields: Vec<&str> = df_lines[1].split_whitespace().collect();
        if fields.len() >= 5 {
            let size = fields[1].to_string();
            let used = fields[2].to_string();
            let use_pct = fields[4].to_string();
            disk_info = Some((size, used, use_pct));
        }
    }

    // CPU & Net Delta Parse
    let cpu_net_split: Vec<&str> = cpu_net.split("===SPLIT===").collect();
    if cpu_net_split.len() < 2 {
        anyhow::bail!("Invalid CPU/Net delta measurement");
    }
    let cn1 = cpu_net_split[0];
    let cn2 = cpu_net_split[1];

    let cn1_split: Vec<&str> = cn1.split("===NET===").collect();
    let cn2_split: Vec<&str> = cn2.split("===NET===").collect();

    if cn1_split.len() < 2 || cn2_split.len() < 2 {
        anyhow::bail!("Invalid network section separator");
    }

    let cpu1_raw = cn1_split[0];
    let net1_raw = cn1_split[1];
    let cpu2_raw = cn2_split[0];
    let net2_raw = cn2_split[1];

    let parse_cpu = |raw: &str| -> Option<(u64, u64)> {
        for line in raw.lines() {
            let line = line.trim();
            if line.starts_with("cpu ") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 5 {
                    let user: u64 = parts[1].parse().unwrap_or(0);
                    let nice: u64 = parts[2].parse().unwrap_or(0);
                    let system: u64 = parts[3].parse().unwrap_or(0);
                    let idle: u64 = parts[4].parse().unwrap_or(0);
                    let iowait: u64 = parts.get(5).and_then(|s| s.parse().ok()).unwrap_or(0);
                    let irq: u64 = parts.get(6).and_then(|s| s.parse().ok()).unwrap_or(0);
                    let softirq: u64 = parts.get(7).and_then(|s| s.parse().ok()).unwrap_or(0);
                    let steal: u64 = parts.get(8).and_then(|s| s.parse().ok()).unwrap_or(0);

                    let total = user + nice + system + idle + iowait + irq + softirq + steal;
                    return Some((idle, total));
                }
            }
        }
        None
    };

    let cpu_usage_pct = match (parse_cpu(cpu1_raw), parse_cpu(cpu2_raw)) {
        (Some((idle1, total1)), Some((idle2, total2))) => {
            let diff_idle = idle2.saturating_sub(idle1);
            let diff_total = total2.saturating_sub(total1);
            if diff_total > 0 {
                Some(100.0 * (1.0 - (diff_idle as f64 / diff_total as f64)))
            } else {
                Some(0.0)
            }
        }
        _ => None,
    };

    let parse_net = |raw: &str| -> HashMap<String, (u64, u64)> {
        let mut map = HashMap::new();
        for line in raw.lines() {
            let line = line.trim();
            if line.contains(':') {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() == 2 {
                    let iface = parts[0].trim().to_string();
                    if iface == "lo" {
                        continue;
                    }
                    let stats: Vec<&str> = parts[1].split_whitespace().collect();
                    if stats.len() >= 9 {
                        let rx: u64 = stats[0].parse().unwrap_or(0);
                        let tx: u64 = stats[8].parse().unwrap_or(0);
                        map.insert(iface, (rx, tx));
                    }
                }
            }
        }
        map
    };

    let net1 = parse_net(net1_raw);
    let net2 = parse_net(net2_raw);

    let mut network_interfaces = Vec::new();
    for (iface, (rx2, tx2)) in &net2 {
        if let Some((rx1, tx1)) = net1.get(iface) {
            let diff_rx = rx2.saturating_sub(*rx1);
            let diff_tx = tx2.saturating_sub(*tx1);
            let rx_rate = (diff_rx as f64) / 1024.0 / 0.2;
            let tx_rate = (diff_tx as f64) / 1024.0 / 0.2;
            network_interfaces.push(NetworkInterfaceStatus {
                name: iface.clone(),
                rx_bytes: *rx2,
                tx_bytes: *tx2,
                rx_rate_kbps: rx_rate,
                tx_rate_kbps: tx_rate,
            });
        }
    }

    Ok((
        NodeResourceStatus {
            cpu_usage_pct,
            mem_total_mb,
            mem_used_mb,
            mem_use_pct,
            disk_info,
            network_interfaces,
        },
        (),
    ))
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
