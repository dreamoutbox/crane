use crate::server_interactor::server_interactor_trait::ServerInteractor;

pub fn setup_traefik(
    interactor: &dyn ServerInteractor,
    app_name: &str,
    domain: &str,
    port_start: u16,
    port_end: u16,
) -> anyhow::Result<()> {
    let mut traefik_config = format!(
        "[http.routers.{name}]\n  rule = \"Host(`{domain}`)\"\n  service = \"{name}\"\n  [http.routers.{name}.tls]\n    certResolver = \"letsencrypt\"\n\n[http.services.{name}.loadBalancer]\n",
        name = app_name,
        domain = domain,
    );
    for port in port_start..port_end {
        traefik_config.push_str(&format!(
            "  [[http.services.{name}.loadBalancer.servers]]\n    url = \"http://127.0.0.1:{port}\"\n",
            name = app_name,
            port = port
        ));
    }
    let traefik_dir = "/etc/traefik/dynamic";
    interactor.cmd(&format!("sudo mkdir -p '{}'", traefik_dir))?;
    let temp_traefik_path = format!("/tmp/{}.toml", app_name);
    interactor.create_file(&temp_traefik_path, &traefik_config)?;
    let dest_traefik_path = format!("{}/{}.toml", traefik_dir, app_name);
    interactor.cmd(&format!("sudo mv '{}' '{}'", temp_traefik_path, dest_traefik_path))?;
    interactor.cmd(&format!("sudo chown root:root '{}'", dest_traefik_path))?;
    interactor.cmd(&format!("sudo chmod 644 '{}'", dest_traefik_path))?;
    if let Err(e) = interactor.cmd("sudo systemctl reload traefik") {
        println!(
            "Warning: failed to reload traefik (it might not be running yet): {}",
            e
        );
    }
    Ok(())
}
