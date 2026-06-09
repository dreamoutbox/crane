use crate::{config, deployer::PYTHON_ZIP_APP_SCRIPT};

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
        std::env::temp_dir().join(format!("crane-zip-helper-{}-{}.py", app.name, *datetime));
    std::fs::write(&python_script_path, PYTHON_ZIP_APP_SCRIPT)?;

    let python_bin = if cfg!(windows) { "python" } else { "python3" };

    let mut zip_cmd = std::process::Command::new(python_bin);

    zip_cmd
        .arg(&python_script_path)
        .arg(&zip_path)
        .arg(&dir_to_deploy);

    for ignore in &ignores {
        zip_cmd.arg(ignore);
    }

    let status = match zip_cmd.status() {
        Ok(status) => status,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && python_bin == "python3" => {
            let mut fallback_cmd = std::process::Command::new("python");
            fallback_cmd
                .arg(&python_script_path)
                .arg(&zip_path)
                .arg(&dir_to_deploy);
            for ignore in ignores {
                fallback_cmd.arg(ignore);
            }
            fallback_cmd.status()?
        }
        Err(e) => return Err(e.into()),
    };

    let _ = std::fs::remove_file(&python_script_path);

    if !status.success() {
        anyhow::bail!("Failed to create zip archive of {:?}", dir_to_deploy);
    }

    Ok(zip_path)
}
