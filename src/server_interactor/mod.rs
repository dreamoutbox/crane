use crate::ssh::SSHSession;

pub mod debian;
pub mod server_interactor_trait;

pub fn get_interactor(
    ssh: SSHSession,
) -> anyhow::Result<Box<dyn server_interactor_trait::ServerInteractor>> {
    let distro = crate::helper::server::get_server_distro(&ssh)?;
    match distro.as_str() {
        "debian" | "ubuntu" => Ok(Box::new(debian::DebianInteractor::new(ssh))),
        other => anyhow::bail!("Unsupported server distribution: {}", other),
    }
}
