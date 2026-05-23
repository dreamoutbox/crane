use crate::server_interactor::server_interactor_trait::ServerInteractor;

pub fn setup_systemd_template(
    interactor: &dyn ServerInteractor,
    app_name: &str,
    deploy_user: &str,
) -> anyhow::Result<()> {
    let systemd_template = format!(
        "[Unit]\nDescription=crane managed: %p instance on port %i\nAfter=network.target\n\n[Service]\nType=simple\nUser={deploy_user}\nWorkingDirectory=/opt/{appname}\nExecStart=/opt/{appname}/current --port %i\nEnvironmentFile=/etc/crane/{appname}/.env\nRestart=on-failure\nRestartSec=5\nNoNewPrivileges=true\nProtectSystem=strict\nProtectHome=true\nPrivateTmp=true\n\n[Install]\nWantedBy=multi-user.target\n",
        deploy_user = deploy_user,
        appname = app_name,
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
