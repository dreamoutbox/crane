use crate::ssh::SSHSession;

pub mod debian;
pub mod server_interactor_trait;

/// get server distro from ssh session
pub fn get_server_distro(ssh: &SSHSession) -> anyhow::Result<String> {
    // check is server online
    if !ssh.ping() {
        anyhow::bail!("Server is offline or unreachable via SSH");
    }

    // Read the ID field from /etc/os-release on the server
    let distro = ssh.run_cmd("grep -E '^ID=' /etc/os-release | cut -d= -f2 | tr -d '\"'")?;
    // println!("\t\tDEBUG distro output: {}", distro.stdout);
    Ok(distro.stdout.trim().to_lowercase())
}

/// wrapper to get server interactor from ssh session
pub fn get_server_interactor(
    ssh: SSHSession,
) -> anyhow::Result<Box<dyn server_interactor_trait::ServerInteractor>> {
    let distro = get_server_distro(&ssh)?;

    get_interactor_for_distro(ssh, &distro)
}

/// Build an interactor using a pre-detected distro string (skips SSH detection).
fn get_interactor_for_distro(
    ssh: SSHSession,
    distro: &str,
) -> anyhow::Result<Box<dyn server_interactor_trait::ServerInteractor>> {
    match distro {
        "debian" | "ubuntu" => Ok(Box::new(debian::DebianInteractor::new(ssh))),
        other => anyhow::bail!("Unsupported server distribution: {}", other),
    }
}
