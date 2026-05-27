use crate::server_interactor::server_interactor_trait::ServerInteractor;

pub fn install_traefik(interactor: &dyn ServerInteractor) -> anyhow::Result<()> {
    // Check if Traefik binary is already installed
    let check = interactor.cmd("which traefik");
    let binary_installed = check.is_ok() && !check.unwrap().stdout.trim().is_empty();

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
  websecure:
    address: ":443"
  internal:
    address: "127.0.0.1:8080"

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

    interactor.create_file("/etc/traefik/traefik.yml", static_config)?;
    interactor.cmd("sudo chown root:root /etc/traefik/traefik.yml")?;
    interactor.cmd("sudo chmod 644 /etc/traefik/traefik.yml")?;

    // Create systemd service for Traefik
    let service_config = r#"
[Unit]
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

    interactor.create_file("/etc/systemd/system/traefik.service", service_config)?;
    interactor.cmd("sudo chown root:root /etc/systemd/system/traefik.service")?;
    interactor.cmd("sudo chmod 644 /etc/systemd/system/traefik.service")?;

    // Reload systemd and restart Traefik (to apply static config changes if already running)
    interactor.cmd("sudo systemctl daemon-reload")?;
    interactor.cmd("sudo systemctl enable traefik")?;
    interactor.cmd("sudo systemctl restart traefik")?;

    Ok(())
}
