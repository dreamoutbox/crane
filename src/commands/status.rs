use crate::helper::keys::find_private_key_for_user;
use crate::server_interactor::get_server_interactor;
use crate::ssh::SSHSession;
use std::collections::{BTreeSet, HashMap};

struct SystemdUnitState {
    port: u16,
    active_state: String,
    sub_state: String,
}

pub struct InstanceStatus {
    pub port: u16,
    pub systemd_active: bool,
    pub systemd_substate: String,
    pub http_status: Option<u16>,
}

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

pub struct NodeStatusResult {
    pub node_name: String,
    pub node_host: String,
    pub status: Result<(NodeResourceStatus, Vec<InstanceStatus>), String>,
}

pub fn run(config: &crate::config::Config, app_name: &str) -> anyhow::Result<()> {
    // Find requested app configuration by key or name
    let (_app_id, app) = config
        .app
        .iter()
        .find(|(id, a)| *id == app_name || a.name == app_name)
        .ok_or_else(|| anyhow::anyhow!("App '{}' not found in configuration", app_name))?;

    // Find all app nodes
    let app_nodes: Vec<_> = config
        .nodes
        .iter()
        .filter(|n| n.roles.contains(&"app".to_string()))
        .cloned()
        .collect();

    if app_nodes.is_empty() {
        anyhow::bail!("No nodes with the 'app' role found in configuration");
    }

    // Determine the external URL
    let external_url = if app.domain.is_some() {
        format!("https://{}", app.domain.as_ref().unwrap())
    } else if let Some(ref d) = config.domain {
        format!("https://{}", d.domain_name)
    } else {
        format!("http://{}", app_nodes[0].public_ip)
    };

    println!(
        "Querying status for app '{}' on {} node(s)...",
        app.name,
        app_nodes.len()
    );

    // Determine configured ports
    let port_end = app.port_start + app.instances as u16;
    let mut configured_ports = BTreeSet::new();
    for port in app.port_start..port_end {
        configured_ports.insert(port);
    }

    let health_path = app.health_check_path.as_deref().unwrap_or("/health");

    // Spawn status gathering in parallel threads
    let mut handles = vec![];
    for node in &app_nodes {
        let node_clone = node.clone();
        let config_clone = config.clone();
        let app_name_clone = app.name.clone();
        let configured_ports_clone = configured_ports.clone();
        let health_path_clone = health_path.to_string();

        let handle = std::thread::spawn(move || -> NodeStatusResult {
            let result = (|| -> anyhow::Result<(NodeResourceStatus, Vec<InstanceStatus>)> {
                let private_key = find_private_key_for_user(&node_clone.user, &config_clone)?;

                let ssh = SSHSession::new(
                    node_clone.host.clone(),
                    node_clone.user.clone(),
                    private_key,
                    Some(node_clone.port),
                );

                let interactor = get_server_interactor(ssh)?;

                // Query service instances from systemd to dynamically discover active ports
                let systemd_cmd = format!(
                    "systemctl list-units --type=service --all \"{}@*.service\" --no-legend --no-pager",
                    app_name_clone
                );
                let systemd_output = interactor.cmd(&systemd_cmd)?;

                let mut systemd_units = Vec::new();
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
                                    let port_str = &unit_name[at_idx + 1..dot_idx];
                                    if let Ok(port) = port_str.parse::<u16>() {
                                        let active_state = parts[2].to_string();
                                        let sub_state = parts[3].to_string();

                                        systemd_units.push(SystemdUnitState {
                                            port,
                                            active_state,
                                            sub_state,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }

                // Determine ports to query (strictly based on configured instances)
                let all_ports = configured_ports_clone.clone();

                // Chained curl commands for HTTP status checks
                let mut curl_cmds = Vec::new();
                for port in &all_ports {
                    curl_cmds.push(format!(
                        "curl -s -o /dev/null -w \"PORT:{}:STATUS:%{{http_code}}\\n\" http://127.0.0.1:{}{} || echo \"PORT:{}:STATUS:000\"",
                        port, port, health_path_clone, port
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
                let (mem_used_mb, mem_total_mb, mem_use_pct) =
                    match (mem_total_kb, mem_available_kb) {
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
                                let iowait: u64 =
                                    parts.get(5).and_then(|s| s.parse().ok()).unwrap_or(0);
                                let irq: u64 =
                                    parts.get(6).and_then(|s| s.parse().ok()).unwrap_or(0);
                                let softirq: u64 =
                                    parts.get(7).and_then(|s| s.parse().ok()).unwrap_or(0);
                                let steal: u64 =
                                    parts.get(8).and_then(|s| s.parse().ok()).unwrap_or(0);

                                let total =
                                    user + nice + system + idle + iowait + irq + softirq + steal;
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

                let resource_status = NodeResourceStatus {
                    cpu_usage_pct,
                    mem_total_mb,
                    mem_used_mb,
                    mem_use_pct,
                    disk_info,
                    network_interfaces,
                };

                // Parse Curls Status
                let mut http_statuses = HashMap::new();
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
                let mut instances_status = Vec::new();
                for port in all_ports {
                    let systemd_state = systemd_units.iter().find(|u| u.port == port);
                    let (sys_active, sys_sub) = match systemd_state {
                        Some(u) => (u.active_state.clone(), u.sub_state.clone()),
                        None => ("inactive".to_string(), "not-found".to_string()),
                    };
                    let http_status = http_statuses.get(&port).copied();

                    instances_status.push(InstanceStatus {
                        port,
                        systemd_active: sys_active == "active",
                        systemd_substate: sys_sub,
                        http_status,
                    });
                }

                Ok((resource_status, instances_status))
            })();

            NodeStatusResult {
                node_name: node_clone.name.clone(),
                node_host: node_clone.host.clone(),
                status: result.map_err(|e| e.to_string()),
            }
        });
        handles.push(handle);
    }

    let mut nodes_results = Vec::new();
    for handle in handles {
        if let Ok(res) = handle.join() {
            nodes_results.push(res);
        }
    }

    // Calculate overall status
    let mut total_instances = 0;
    let mut healthy_instances = 0;
    let mut has_any_node_offline = false;

    for node_res in &nodes_results {
        match &node_res.status {
            Err(_) => {
                has_any_node_offline = true;
            }
            Ok((_, instances)) => {
                total_instances += instances.len();
                healthy_instances += instances
                    .iter()
                    .filter(|i| i.systemd_active && i.http_status == Some(200))
                    .count();
            }
        }
    }

    let (status_text, status_color) =
        if total_instances > 0 && healthy_instances == total_instances && !has_any_node_offline {
            ("HEALTHY", "\x1b[1;32m")
        } else if healthy_instances > 0 || (total_instances > 0 && !has_any_node_offline) {
            ("DEGRADED", "\x1b[1;33m")
        } else {
            ("UNHEALTHY", "\x1b[1;31m")
        };

    // Print Header
    println!(
        "\n\x1b[1;36m======================================================================\x1b[0m"
    );
    println!("\x1b[1;36mApp Status Report: {}\x1b[0m", app.name);
    println!("External URL:    \x1b[1;34m{}\x1b[0m", external_url);
    println!(
        "Port Range:      {} - {}",
        app.port_start,
        app.port_end
            .map(|p| p.to_string())
            .unwrap_or_else(|| "N/A".to_string())
    );
    println!("Overall Status:  {}{}\x1b[0m", status_color, status_text);
    println!(
        "\x1b[1;36m======================================================================\x1b[0m\n"
    );

    // Print Node Statuses
    for node_res in nodes_results {
        let node_index = app_nodes
            .iter()
            .position(|n| n.name == node_res.node_name)
            .unwrap_or(0);

        println!(
            "\x1b[1;35mNode: {} ({})\x1b[0m",
            node_res.node_name, node_res.node_host
        );
        println!(
            "\x1b[1;35m----------------------------------------------------------------------\x1b[0m"
        );

        match node_res.status {
            Err(err_str) => {
                println!("  \x1b[1;31m[OFFLINE] Unreachable: {}\x1b[0m\n", err_str);
            }
            Ok((resource, instances)) => {
                // CPU / Memory
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
                    println!("  CPU/Memory: [Unavailable]");
                }

                // Disk
                if let Some((size, used, pct)) = resource.disk_info {
                    println!("  Disk (/):   {} / {} ({})", used, size, pct);
                } else {
                    println!("  Disk (/):   [Unavailable]");
                }

                // Network
                println!("  Network:");
                if resource.network_interfaces.is_empty() {
                    println!("    No active interfaces found.");
                } else {
                    for net in &resource.network_interfaces {
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

                // Instances
                println!("\n  Instances:");
                if instances.is_empty() {
                    println!("    No instances found.");
                } else {
                    for inst in &instances {
                        let sys_status = if inst.systemd_active {
                            "\x1b[32mActive\x1b[0m"
                        } else {
                            &format!("\x1b[31mInactive ({})\x1b[0m", inst.systemd_substate)
                        };

                        let http_status = match inst.http_status {
                            Some(200) => "\x1b[32mHTTP 200 OK\x1b[0m".to_string(),
                            Some(code) if code > 0 => format!("\x1b[31mHTTP {}\x1b[0m", code),
                            _ => "\x1b[31mHTTP Connection Failed\x1b[0m".to_string(),
                        };

                        let is_healthy = inst.systemd_active && inst.http_status == Some(200);
                        let bullet = if is_healthy {
                            "\x1b[32m✔\x1b[0m"
                        } else {
                            "\x1b[31m✘\x1b[0m"
                        };

                        let port_index = (inst.port - app.port_start) as usize;
                        let instance_id = node_index * (app.instances as usize) + port_index + 1;

                        println!(
                            "    {} {}@{} (Port {}): {} | {}",
                            bullet, app.name, instance_id, inst.port, sys_status, http_status
                        );
                    }
                }
                println!();
            }
        }
    }

    Ok(())
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
