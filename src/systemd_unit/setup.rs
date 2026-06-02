use crate::server_interactor::server_interactor_trait::ServerInteractor;

pub fn setup_systemd_template(
    interactor: &dyn ServerInteractor,
    app_name: &str,
    deploy_user: &str,
    entrypoint: &str,
) -> anyhow::Result<()> {
    let service_file_path = format!("/etc/systemd/system/{}@.service", app_name);
    let clean_entrypoint = entrypoint.trim_start_matches("./");
    let env_file = format!("/app_config/{}/.env", app_name);
    let working_dir = format!("/app/{}", app_name);
    // let exec_start = format!("/app/{}/{} --port %i", app_name, clean_entrypoint);
    let exec_start = format!("/app/{}/{}", app_name, clean_entrypoint);

    println!(
        "\t[{app_name}] Register Service:
\t\tPath={service_file_path}
\t\tUser={deploy_user}
\t\tWorkingDirectory={working_dir}
\t\tExecStart={exec_start}
\t\tEnvFile={env_file}"
    );

    let systemd_data = format!(
        r#"[Unit]
Description=crane managed: %p instance on port %i
After=network.target

[Service]
Type=simple
User={deploy_user}
WorkingDirectory={working_dir}
ExecStart={exec_start}
EnvironmentFile={env_file}
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
        working_dir = working_dir,
        exec_start = exec_start,
        env_file = env_file,
    );

    interactor.create_file(&service_file_path, &systemd_data)?;

    interactor.chown(&service_file_path, "root", "root")?;
    interactor.chmod(&service_file_path, "644")?;

    let _ = interactor.service_daemon_reload()?;

    Ok(())
}
