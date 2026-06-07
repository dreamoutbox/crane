use crate::{server_interactor::server_interactor_trait::ServerInteractor, ssh::SSHSession};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

pub mod debian;
pub mod server_interactor_trait;
pub mod server_path;

pub struct ConnectionRegistry {
    pub config: crate::config::Config,
    pub connections: Mutex<HashMap<String, Arc<dyn ServerInteractor + Send + Sync>>>,
}

static REGISTRY: RwLock<Option<ConnectionRegistry>> = RwLock::new(None);

pub fn init_registry(config: crate::config::Config) {
    let mut reg = REGISTRY.write().unwrap();
    *reg = Some(ConnectionRegistry {
        config,
        connections: Mutex::new(HashMap::new()),
    });
}

/// wrapper to get server interactor from node name
pub fn get_server_interactor(
    node_name: &str,
) -> anyhow::Result<Arc<dyn ServerInteractor + Send + Sync>> {
    let reg_guard = REGISTRY.read().unwrap();

    let registry = reg_guard.as_ref().ok_or_else(|| {
        anyhow::anyhow!("Connection registry not initialized. Please load configuration first.")
    })?;

    let mut connections = registry.connections.lock().unwrap();
    if let Some(interactor) = connections.get(node_name) {
        return Ok(interactor.clone());
    }

    // Not cached, find the node config
    let node = registry
        .config
        .nodes
        .iter()
        .find(|n| {
            n.name == node_name
                || n.name == node_name
                || n.internal_ip == node_name
                || n.public_ip == node_name
        })
        .ok_or_else(|| anyhow::anyhow!("Node '{}' not found in configuration", node_name))?;

    let private_key = crate::helper::keys::find_private_key_for_user(&node.user, &registry.config)?;
    let ssh = SSHSession::new(
        node.ssh_ip.clone(),
        node.user.clone(),
        private_key,
        Some(node.ssh_port),
    );

    let distro = get_server_distro(&ssh)?;
    let interactor = get_interactor_for_distro(ssh, node.sudo_pass.clone(), &distro)?;
    let interactor_arc: Arc<dyn ServerInteractor + Send + Sync> = Arc::from(interactor);

    connections.insert(node_name.to_string(), interactor_arc.clone());

    Ok(interactor_arc)
}

/// get server distro from ssh session
fn get_server_distro(ssh: &SSHSession) -> anyhow::Result<String> {
    // check is server online
    if !ssh.ping() {
        anyhow::bail!("Server is offline or unreachable via SSH");
    }

    // Read the ID field from /etc/os-release on the server
    let distro = ssh.run_cmd("grep -E '^ID=' /etc/os-release | cut -d= -f2 | tr -d '\"'")?;
    // println!("\t\tDEBUG distro output: {}", distro.stdout);
    Ok(distro.stdout.trim().to_lowercase())
}

/// Build an interactor using a pre-detected distro string (skips SSH detection).
fn get_interactor_for_distro(
    ssh: SSHSession,
    sudo_pass: Option<String>,
    distro: &str,
) -> anyhow::Result<Box<dyn ServerInteractor + Send + Sync>> {
    match distro {
        "debian" | "ubuntu" => Ok(Box::new(debian::DebianInteractor::new(ssh, sudo_pass))),
        other => anyhow::bail!("Unsupported server distribution: {}", other),
    }
}
