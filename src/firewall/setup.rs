use crate::config::Config;
use crate::server_interactor::server_interactor_trait::ServerInteractor;

pub fn setup_firewall(interactor: &dyn ServerInteractor, config: &Config) -> anyhow::Result<()> {
    // 1. Reset firewall rules to defaults
    interactor.firewall_reset()?;

    // 2. Allow SSH before enabling to avoid lockouts
    interactor.firewall_allow_port(22, "tcp", None)?;

    // 3. Enable firewall
    interactor.firewall_enable(true)?;

    // 4. Allow public HTTP/HTTPS traffic
    interactor.firewall_allow_port(80, "tcp", None)?;
    interactor.firewall_allow_port(443, "tcp", None)?;

    // 5. Allow all incoming traffic from internal IPs of all nodes in the cluster
    for node in &config.nodes {
        interactor.firewall_allow_source(&node.internal_ip)?;
    }

    // 6. Reload firewall to apply rules
    interactor.firewall_reload()?;

    Ok(())
}
