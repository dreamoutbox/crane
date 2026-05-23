use crate::ssh::SSHSession;

pub fn get_server_distro(ssh: &SSHSession) -> anyhow::Result<String> {
    // Read the ID field from /etc/os-release on the server
    let distro = ssh.run_cmd("grep -E '^ID=' /etc/os-release | cut -d= -f2 | tr -d '\"'")?;
    // println!("\t\tDEBUG distro output: {}", distro.stdout);
    Ok(distro.stdout.trim().to_lowercase())
}
