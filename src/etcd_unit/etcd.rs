use crate::{config, server_interactor::server_interactor_trait::ServerInteractor};

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

/// etcd_clear_dcs_state
/// Clear DCS (etcd) keys for the cluster to prevent conflicts
pub fn etcd_clear_dcs_state(interactor: &dyn ServerInteractor) {
    let _ = interactor.cmd("sudo env ETCDCTL_API=3 etcdctl del /service/postgres-cluster --prefix");
}
