use crate::server_interactor::server_interactor_trait::ServerInteractor;

pub fn setup_systemd_template(
    interactor: &dyn ServerInteractor,
    app_name: &str,
    deploy_user: &str,
    entrypoint: &str,
) -> anyhow::Result<()> {
    let service_file_path = format!("/etc/systemd/system/{}@.service", app_name);

    let clean_entrypoint = entrypoint.trim_start_matches("./");

    println!(
        "\t[{app_name}] systemd file={service_file_path} ExecStart=/app/{app_name}/{clean_entrypoint}"
    );

    let systemd_data = format!(
        r#"[Unit]
Description=crane managed: %p instance on port %i
After=network.target

[Service]
Type=simple
User={deploy_user}
WorkingDirectory=/app/{appname}
ExecStart=/app/{appname}/{entrypoint} --port %i
EnvironmentFile=/app_config/{appname}/.env
Restart=on-failure
RestartSec=5
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true

[Install]
WantedBy=multi-user.target
"#,
        deploy_user = deploy_user,
        appname = app_name,
        entrypoint = clean_entrypoint,
    );

    interactor.create_file(&service_file_path, &systemd_data)?;

    interactor.chown(&service_file_path, "root", "root")?;
    interactor.chmod(&service_file_path, "644")?;

    let _ = interactor.service_daemon_reload()?;

    let _ = interactor.enable_service(&format!("{}@", app_name));

    Ok(())
}
