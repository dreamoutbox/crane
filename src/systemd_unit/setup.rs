use crate::server_interactor::server_interactor_trait::ServerInteractor;

pub fn setup_systemd_template(
    interactor: &dyn ServerInteractor,
    app_name: &str,
    deploy_user: &str,
    entrypoint: &str,
) -> anyhow::Result<()> {
    let clean_entrypoint = entrypoint.trim_start_matches("./");

    println!("\t[{app_name}] systemd ExecStart=/opt/{app_name}/current/{entrypoint}");

    let systemd_template = format!(
        r#"[Unit]
Description=crane managed: %p instance on port %i
After=network.target

[Service]
Type=simple
User={deploy_user}
WorkingDirectory=/opt/{appname}/current
ExecStart=/opt/{appname}/current/{entrypoint} --port %i
EnvironmentFile=/etc/crane/{appname}/.env
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

    let temp_service_path = format!("/tmp/{}@.service", app_name);
    interactor.create_file(&temp_service_path, &systemd_template)?;
    let dest_service_path = format!("/etc/systemd/system/{}@.service", app_name);

    interactor.cmd(&format!(
        "sudo mv '{}' '{}'",
        temp_service_path, dest_service_path
    ))?;
    interactor.cmd(&format!("sudo chown root:root '{}'", dest_service_path))?;
    interactor.cmd(&format!("sudo chmod 644 '{}'", dest_service_path))?;
    interactor.cmd("sudo systemctl daemon-reload")?;

    let _ = interactor.cmd(&format!("sudo systemctl enable '{}@'", app_name));

    Ok(())
}
