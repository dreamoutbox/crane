use crate::server_interactor::server_interactor_trait::ServerInteractor;

pub fn setup_haproxy(
    interactor: &dyn ServerInteractor,
    primary_ip: &str,
    follower_ips: &[String],
) -> anyhow::Result<()> {
    println!("\tSetting up HAProxy in front of the PostgreSQL cluster...");

    println!("\tInstalling HAProxy...");
    interactor.install_dependencies(vec!["haproxy".to_string()])?;

    let mut haproxy_cfg = format!(
        r#"
global
    log /dev/log local0
    log /dev/log local1 notice
    chroot /var/lib/haproxy
    user haproxy
    group haproxy
    daemon

defaults
    log global
    mode tcp
    option tcplog
    option dontlognull
    retries 3
    timeout connect 5000ms
    timeout client 50000ms
    timeout server 50000ms

frontend postgres_front
    bind *:5000
    mode tcp
    default_backend postgres_back

backend postgres_back
    mode tcp
    option tcp-check
    server postgres-primary {}:5432 check


"#,
        primary_ip
    );

    for (idx, follower) in follower_ips.iter().enumerate() {
        haproxy_cfg.push_str(&format!(
            "    server postgres-follower-{} {}:5432 check backup\n",
            idx + 1,
            follower
        ));
    }

    haproxy_cfg.push_str("\nfrontend postgres_follower_front\n");
    haproxy_cfg.push_str("    bind *:5001\n");
    haproxy_cfg.push_str("    mode tcp\n");
    haproxy_cfg.push_str("    default_backend postgres_follower_back\n\n");
    haproxy_cfg.push_str("backend postgres_follower_back\n");
    haproxy_cfg.push_str("    mode tcp\n");
    haproxy_cfg.push_str("    balance roundrobin\n");
    haproxy_cfg.push_str("    option tcp-check\n");

    if follower_ips.is_empty() {
        haproxy_cfg.push_str(&format!(
            "    server postgres-primary {}:5432 check\n",
            primary_ip
        ));
    } else {
        for (idx, follower) in follower_ips.iter().enumerate() {
            haproxy_cfg.push_str(&format!(
                "    server postgres-follower-{} {}:5432 check\n",
                idx + 1,
                follower
            ));
        }
    }

    println!("\tWriting HAProxy configuration...");
    interactor.create_file("/etc/haproxy/haproxy.cfg", &haproxy_cfg)?;
    interactor.cmd("sudo chown root:root /etc/haproxy/haproxy.cfg")?;
    interactor.cmd("sudo chmod 644 /etc/haproxy/haproxy.cfg")?;

    println!("\tRestarting and enabling HAProxy service...");
    interactor.cmd("sudo systemctl daemon-reload")?;
    interactor.cmd("sudo systemctl enable haproxy")?;
    interactor.cmd("sudo systemctl restart haproxy")?;

    Ok(())
}
