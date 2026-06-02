use crate::{
    config, deployer::python_zip_script::PYTHON_ZIP_SCRIPT,
    server_interactor::server_interactor_trait::ServerInteractor,
};

pub fn deploy_zip_app(
    app: &config::AppConfig,
    datetime: &String,
    dir_to_deploy: std::path::PathBuf,
) -> Result<std::path::PathBuf, anyhow::Error> {
    let zip_path =
        std::env::temp_dir().join(format!("crane-deploy-{}-{}.zip", app.name, *datetime));
    let craneignore_path = dir_to_deploy.join(".craneignore");
    let mut ignores = vec![];
    if craneignore_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&craneignore_path) {
            for line in content.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() && !trimmed.starts_with('#') {
                    ignores.push(trimmed.to_string());
                }
            }
        }
    }
    ignores.push(".git".to_string());

    let python_script_path =
        std::env::temp_dir().join(format!("crane-zip-helper-{}.py", *datetime));
    std::fs::write(&python_script_path, PYTHON_ZIP_SCRIPT)?;
    let mut zip_cmd = std::process::Command::new("python3");
    zip_cmd
        .arg(&python_script_path)
        .arg(&zip_path)
        .arg(&dir_to_deploy);
    for ignore in ignores {
        zip_cmd.arg(ignore);
    }
    let status = zip_cmd.status()?;
    let _ = std::fs::remove_file(&python_script_path);
    if !status.success() {
        anyhow::bail!("Failed to create zip archive of {:?}", dir_to_deploy);
    }

    Ok(zip_path)
}

/// Idempotently add/update `/etc/hosts` entries on the remote server.
/// For each (hostname, ip) pair: replace existing line if present, else append.
pub fn deploy_update_etc_hosts(
    interactor: &dyn ServerInteractor,
    entries: &[(String, String)], // (hostname, ip)
) -> anyhow::Result<()> {
    for (hostname, ip) in entries {
        // println!("\t- pointing {} -> {}", hostname, ip);
        // Remove old entry for this hostname, then append the new one.
        // We use a temp file approach to avoid sed -i portability issues.
        let cmd = format!(
            r#"sudo sh -c 'grep -v " {hostname}" /etc/hosts > /tmp/hosts.tmp && echo "{ip} {hostname}" >> /tmp/hosts.tmp && cp /tmp/hosts.tmp /etc/hosts && rm /tmp/hosts.tmp'"#,
            hostname = hostname,
            ip = ip,
        );
        interactor.cmd(&cmd)?;
    }
    Ok(())
}
