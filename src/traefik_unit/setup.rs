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
[http.routers.{name}-redirect]
  rule = "Host(`{domain}`)"
  entryPoints = ["web"]
  service = "{name}"
  middlewares = ["{name}-redirect"]

[http.middlewares.{name}-redirect.redirectScheme]
  scheme = "https"
  permanent = true

[http.routers.{name}-external]
  rule = "Host(`{domain}`)"
  entryPoints = ["websecure"]
  service = "{name}"
  [http.routers.{name}-external.tls]
    certResolver = "letsencrypt"

[http.routers.{name}-internal]
  rule = "Host(`{name}`)"
  entryPoints = ["internal", "web"]
  service = "{name}"

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
