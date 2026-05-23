use crate::server_interactor::server_interactor_trait::ServerInteractor;

pub fn setup_traefik(
    interactor: &dyn ServerInteractor,
    app_name: &str,
    domain: &str,
    port_start: u16,
    port_end: u16,
    health_check_path: &str,
) -> anyhow::Result<()> {
    // 2. Generate and write the dynamic configuration
    let mut traefik_config = format!(
        r#"
[http.routers.{name}]
  rule = "Host(`{domain}`)"
  service = "{name}"
  [http.routers.{name}.tls]
    certResolver = "letsencrypt"

[http.services.{name}.loadBalancer]
"#,
        name = app_name,
        domain = domain,
    );
    for port in port_start..port_end {
        traefik_config.push_str(&format!(
            r#"  
[[http.services.{name}.loadBalancer.servers]]
url = "http://127.0.0.1:{port}"
"#,
            name = app_name,
            port = port
        ));
    }

    // Add active health check configuration
    traefik_config.push_str(&format!(
        r#"
[http.services.{name}.loadBalancer.healthCheck]
path = "{health_path}"
interval = "2s"
timeout = "1s"
"#,
        name = app_name,
        health_path = health_check_path,
    ));

    let traefik_dir = "/etc/traefik/dynamic";
    interactor.cmd(&format!("sudo mkdir -p '{}'", traefik_dir))?;
    let temp_traefik_path = format!("/tmp/{}.toml", app_name);
    interactor.create_file(&temp_traefik_path, &traefik_config)?;
    let dest_traefik_path = format!("{}/{}.toml", traefik_dir, app_name);
    interactor.cmd(&format!(
        "sudo mv '{}' '{}'",
        temp_traefik_path, dest_traefik_path
    ))?;
    interactor.cmd(&format!("sudo chown root:root '{}'", dest_traefik_path))?;
    interactor.cmd(&format!("sudo chmod 644 '{}'", dest_traefik_path))?;

    if let Err(e) = interactor.cmd("sudo systemctl reload traefik") {
        println!(
            "Warning: failed to reload traefik (it might not be running yet): {}",
            e
        );

        let traefik_start_output = interactor.cmd("sudo systemctl start traefik")?;
        println!("traefik_start_output: {}", traefik_start_output);
    }

    Ok(())
}

pub fn install_traefik(interactor: &dyn ServerInteractor) -> anyhow::Result<()> {
    // Check if Traefik binary is already installed
    let check = interactor.cmd("which traefik");
    let binary_installed = check.is_ok() && !check.unwrap().trim().is_empty();

    if !binary_installed {
        println!("Installing Traefik on remote server...");

        // Download and extract Traefik AMD64 Linux release
        let download_cmd = "wget -q https://github.com/traefik/traefik/releases/download/v3.0.0/traefik_v3.0.0_linux_amd64.tar.gz -O /tmp/traefik.tar.gz || curl -L https://github.com/traefik/traefik/releases/download/v3.0.0/traefik_v3.0.0_linux_amd64.tar.gz -o /tmp/traefik.tar.gz";
        interactor.cmd(download_cmd)?;
        interactor.cmd("tar -xzf /tmp/traefik.tar.gz -C /tmp/ traefik")?;
        interactor.cmd("sudo mv /tmp/traefik /usr/local/bin/traefik")?;
        interactor.cmd("sudo chmod +x /usr/local/bin/traefik")?;

        let _ = interactor.cmd("rm -f /tmp/traefik.tar.gz");
    }

    // Create configuration directories
    interactor.cmd("sudo mkdir -p /etc/traefik/dynamic")?;

    // Create Traefik static configuration with entrypoints, redirects, and ACME resolvers
    let static_config = r#"
entryPoints:
  web:
    address: ":80"
    http:
      redirections:
        entryPoint:
          to: "websecure"
          scheme: "https"
  websecure:
    address: ":443"

providers:
  file:
    directory: "/etc/traefik/dynamic"
    watch: true

certificatesResolvers:
  letsencrypt:
    acme:
      email: "dev@example.com"
      storage: "/etc/traefik/acme.json"
      httpChallenge:
        entryPoint: "web"
"#;

    interactor.create_file("/tmp/traefik.yml", static_config)?;
    interactor.cmd("sudo mv /tmp/traefik.yml /etc/traefik/traefik.yml")?;
    interactor.cmd("sudo chown root:root /etc/traefik/traefik.yml")?;
    interactor.cmd("sudo chmod 644 /etc/traefik/traefik.yml")?;

    // Create systemd service for Traefik
    let service_config = r#"[Unit]
Description=Traefik
After=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/traefik --configfile=/etc/traefik/traefik.yml
ExecReload=/bin/true
Restart=always

[Install]
WantedBy=multi-user.target
"#;

    interactor.create_file("/tmp/traefik.service", service_config)?;
    interactor.cmd("sudo mv /tmp/traefik.service /etc/systemd/system/traefik.service")?;
    interactor.cmd("sudo chown root:root /etc/systemd/system/traefik.service")?;
    interactor.cmd("sudo chmod 644 /etc/systemd/system/traefik.service")?;

    // Reload systemd and restart Traefik (to apply static config changes if already running)
    interactor.cmd("sudo systemctl daemon-reload")?;
    interactor.cmd("sudo systemctl enable traefik")?;
    interactor.cmd("sudo systemctl restart traefik")?;

    Ok(())
}
