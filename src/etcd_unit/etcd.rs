use crate::{
    config, helper::server::wait_for_service_status,
    server_interactor::server_interactor_trait::ServerInteractor,
};

pub fn install_etcd(interactor: &dyn ServerInteractor) -> anyhow::Result<()> {
    let installed = interactor
        .cmd("which etcd")
        .map(|out| out.exit_code == 0)
        .unwrap_or(false);

    if !installed {
        println!("\tInstalling etcd-server and etcd-client...");
        interactor
            .install_dependencies(vec!["etcd-server".to_string(), "etcd-client".to_string()])?;

        interactor.service_daemon_reload()?;
        interactor.enable_service("etcd")?;
    } else {
        println!("\tetcd is already installed.");
    }

    Ok(())
}

pub fn setup_etcd(
    interactor: &dyn ServerInteractor,
    node: &config::NodeConfig,
    pg_nodes: &[config::NodeConfig],
) -> anyhow::Result<()> {
    println!("\tSetup etcd cluster on node {}...", node.name);

    let etcd_configured = interactor
        .cmd("test -f /etc/default/etcd")
        .map(|out| out.exit_code == 0)
        .unwrap_or(false);
    if !etcd_configured {
        // Stop etcd cleanly and remove data directory; wait to ensure it is fully stopped
        let _ = interactor.stop_service("etcd");
        let _ = wait_for_service_status(interactor, "etcd", "inactive", 30);
        let _ = interactor.cmd("sudo rm -rf /var/lib/etcd/");

        // Recreate with correct ownership so the etcd service user can write to it
        interactor.cmd("sudo mkdir -p /var/lib/etcd")?;
        interactor.cmd("sudo chown etcd:etcd /var/lib/etcd")?;
        interactor.cmd("sudo chmod 700 /var/lib/etcd")?;
    }

    let initial_cluster = pg_nodes
        .iter()
        .map(|n| format!("{}=http://{}:2380", n.name, n.internal_ip))
        .collect::<Vec<_>>()
        .join(",");

    // Use "existing" if etcd member data already exists to avoid re-triggering
    // Patroni DCS re-bootstrap (and pg_basebackup) on every redeploy.
    let has_etcd_data = interactor
        .cmd("test -d /var/lib/etcd/default.etcd/member")
        .map(|o| o.exit_code == 0)
        .unwrap_or(false);
    let cluster_state = if has_etcd_data { "existing" } else { "new" };

    let etcd_default = format!(
        r#"
# Member settings
ETCD_NAME="{etcd_name}"
ETCD_DATA_DIR="/var/lib/etcd/default.etcd"
ETCD_LISTEN_PEER_URLS="http://0.0.0.0:2380"
ETCD_LISTEN_CLIENT_URLS="http://0.0.0.0:2379"

# Clustering settings
ETCD_INITIAL_ADVERTISE_PEER_URLS="http://{internal_ip}:2380"
ETCD_INITIAL_CLUSTER="{initial_cluster}"
ETCD_INITIAL_CLUSTER_STATE="{cluster_state}"
ETCD_INITIAL_CLUSTER_TOKEN="etcd-postgres-token"
ETCD_ADVERTISE_CLIENT_URLS="http://{internal_ip}:2379"
"#,
        etcd_name = node.name,
        internal_ip = node.internal_ip,
        initial_cluster = initial_cluster,
        cluster_state = cluster_state,
    );

    // println!("\nETCD CONFIG\n{}\n", &etcd_default);

    let etcd_default_path = "/etc/default/etcd";
    interactor.create_file(etcd_default_path, &etcd_default)?;
    interactor.cmd(&format!("sudo chown root:root '{}'", etcd_default_path))?;
    interactor.cmd(&format!("sudo chmod 644 '{}'", etcd_default_path))?;

    Ok(())
}

/// Start etcd non-blocking. Call after all nodes are configured so the cluster forms together.
pub fn start_etcd(
    node: &config::NodeConfig,
    interactor: &dyn ServerInteractor,
) -> anyhow::Result<()> {
    // Check the node's internal IP (not localhost) so we only skip restart when etcd is
    // actually bound to the correct interface.
    // Checking localhost would pass even if etcd is running
    // with an old config that only binds 127.0.0.1.
    let check_cmd = format!(
        "env ETCDCTL_API=3 etcdctl --endpoints=http://{}:2379 endpoint health",
        node.internal_ip
    );
    let is_healthy = interactor
        .cmd(&check_cmd)
        .map(|o| o.exit_code == 0)
        .unwrap_or(false);

    if is_healthy {
        println!(
            "\tetcd already healthy on node {}, skipping restart",
            node.name
        );
        return Ok(());
    }

    println!("\tStarting etcd service on node {} ...", node.name);
    interactor.restart_service("etcd --no-block")?;

    Ok(())
}

/// Wait for etcd quorum to form by polling all cluster endpoints (internal IPs).
/// This ensures all peers are reachable before Patroni starts, not just localhost.
pub fn wait_for_etcd_cluster(
    interactor: &dyn ServerInteractor,
    pg_nodes: &[config::NodeConfig],
    timeout_secs: u64,
) -> anyhow::Result<()> {
    let endpoints = pg_nodes
        .iter()
        .map(|n| format!("http://{}:2379", n.internal_ip))
        .collect::<Vec<_>>()
        .join(",");

    let health_cmd = format!(
        "env ETCDCTL_API=3 etcdctl --endpoints={} endpoint health",
        endpoints
    );

    // dbg!(&health_cmd);

    let start = std::time::Instant::now();
    let duration = std::time::Duration::from_secs(timeout_secs);

    while start.elapsed() < duration {
        let cmd_result = interactor.cmd(&health_cmd);

        // dbg!(&cmd_result);

        if let Ok(output) = cmd_result {
            if output.exit_code == 0 {
                return Ok(());
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    anyhow::bail!("Timeout waiting for etcd quorum to form")
}
