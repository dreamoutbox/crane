use crate::{
    config,
    helper::keys::find_private_key_for_user,
    server_interactor::{get_server_interactor, server_interactor_trait::ServerInteractor},
    ssh::SSHSession,
};

pub fn setup_haproxy(
    interactor: &dyn ServerInteractor,
    primary_ip: &str,
    follower_ips: &[String],
) -> anyhow::Result<()> {
    println!("\tSetting up HAProxy in front of the PostgreSQL cluster...");

    println!("\tInstalling HAProxy...");
    interactor.install_dependencies(vec!["haproxy".to_string()])?;

    let mut pg_ips = vec![primary_ip.to_string()];
    pg_ips.extend(follower_ips.iter().cloned());

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

frontend postgres_primary_front
    bind *:5000
    mode tcp
    default_backend postgres_primary_back

backend postgres_primary_back
    mode tcp
    option httpchk GET /primary
    http-check expect status 200
    default-server inter 3s fall 3 rise 2 check port 8008 on-marked-down shutdown-sessions
"#
    );

    // List all postgres nodes in the primary backend
    for (idx, ip) in pg_ips.iter().enumerate() {
        haproxy_cfg.push_str(&format!(
            "    server postgres-node-{} {}:5432 check\n",
            idx + 1,
            ip
        ));
    }

    haproxy_cfg.push_str(
        r#"
frontend postgres_replica_front
    bind *:5001
    mode tcp
    default_backend postgres_replica_back

backend postgres_replica_back
    mode tcp
    balance roundrobin
    option httpchk GET /replica
    http-check expect status 200
    default-server inter 3s fall 3 rise 2 check port 8008
"#,
    );

    // List all postgres nodes in the follower backend
    for (idx, ip) in pg_ips.iter().enumerate() {
        haproxy_cfg.push_str(&format!(
            "    server postgres-node-{} {}:5432 check\n",
            idx + 1,
            ip
        ));
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

pub fn setup_haproxy_each_nodes_wrapper(
    config: &config::Config,
    app_nodes: &Vec<config::NodeConfig>,
    leader: config::NodeConfig,
    follower_ips: Vec<String>,
) -> Result<(), anyhow::Error> {
    Ok(for app_node in app_nodes {
        println!("\n\tSetting up HAProxy on app node {}...", app_node.name);

        let private_key = find_private_key_for_user(&app_node.user, config)?;
        let ssh = SSHSession::new(
            app_node.host.clone(),
            app_node.user.clone(),
            private_key,
            Some(app_node.port),
        );
        let interactor = get_server_interactor(ssh)?;

        crate::postgres_unit::haproxy::setup_haproxy(
            &*interactor,
            &leader.internal_ip,
            &follower_ips,
        )?;
    })
}
