use crate::ssh::SSHSession;

pub mod debian;
pub mod server_interactor_trait;

pub fn get_interactor(
    ssh: SSHSession,
) -> anyhow::Result<Box<dyn server_interactor_trait::ServerInteractor>> {
    let distro = crate::helper::server::get_server_distro(&ssh)?;
    get_interactor_for_distro(ssh, &distro)
}

/// Build an interactor using a pre-detected distro string (skips SSH detection).
pub fn get_interactor_for_distro(
    ssh: SSHSession,
    distro: &str,
) -> anyhow::Result<Box<dyn server_interactor_trait::ServerInteractor>> {
    match distro {
        "debian" | "ubuntu" => Ok(Box::new(debian::DebianInteractor::new(ssh))),
        other => anyhow::bail!("Unsupported server distribution: {}", other),
    }
}
