use crate::{config, server_interactor::server_interactor_trait::ServerInteractor};

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
    println!("\tConfiguring etcd cluster on node {}...", node.name);

    // Stop etcd cleanly and remove data directory; wait to ensure it is fully stopped
    let _ = interactor.stop_service("etcd");
    std::thread::sleep(std::time::Duration::from_secs(1));
    let _ = interactor.cmd("sudo rm -rf /var/lib/etcd/");

    // Recreate with correct ownership so the etcd service user can write to it
    interactor.cmd("sudo mkdir -p /var/lib/etcd")?;
    interactor.cmd("sudo chown etcd:etcd /var/lib/etcd")?;
    interactor.cmd("sudo chmod 700 /var/lib/etcd")?;

    let initial_cluster = pg_nodes
        .iter()
        .map(|n| format!("{}=http://{}:2380", n.name, n.internal_ip))
        .collect::<Vec<_>>()
        .join(",");

    let etcd_default = format!(
        r#"
# Member settings
ETCD_NAME="{}"
ETCD_DATA_DIR="/var/lib/etcd/default.etcd"
ETCD_LISTEN_PEER_URLS="http://{}:2380"
ETCD_LISTEN_CLIENT_URLS="http://127.0.0.1:2379,http://{}:2379"

# Clustering settings
ETCD_INITIAL_ADVERTISE_PEER_URLS="http://{}:2380"
ETCD_INITIAL_CLUSTER="{}"
ETCD_INITIAL_CLUSTER_STATE="new"
ETCD_INITIAL_CLUSTER_TOKEN="etcd-postgres-token"
ETCD_ADVERTISE_CLIENT_URLS="http://{}:2379"
"#,
        node.name,
        node.internal_ip,
        node.internal_ip,
        node.internal_ip,
        initial_cluster,
        node.internal_ip
    );

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
    println!("\tStarting etcd service on node {} ...", node.name);

    let _start_etcd_output = interactor.restart_service("etcd --no-block")?;
    // dbg!(start_etcd_output);

    Ok(())
}

/// Wait for etcd quorum to form by polling the health of the endpoint.
pub fn wait_for_etcd_quorum(
    interactor: &dyn ServerInteractor,
    timeout_secs: u64,
) -> anyhow::Result<()> {
    println!("\tWaiting for etcd quorum...");
    let start = std::time::Instant::now();
    let duration = std::time::Duration::from_secs(timeout_secs);

    while start.elapsed() < duration {
        if let Ok(output) = interactor.cmd("env ETCDCTL_API=3 etcdctl endpoint health") {
            if output.exit_code == 0 {
                println!("\tEtcd quorum formed.");
                return Ok(());
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    anyhow::bail!("Timeout waiting for etcd quorum to form")
}
