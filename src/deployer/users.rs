use crate::{config, server_interactor::server_interactor_trait::ServerInteractor};

pub fn deploy_setup_users(
    app: &config::AppConfig,
    config: &config::Config,
    node_interactor: &Box<dyn ServerInteractor>,
) -> anyhow::Result<()> {
    if let Some(ref users) = config.users {
        if let Some(user_config) = users.iter().find(|u| u.name == app.deploy_user) {
            let mut authorized_keys = Vec::new();
            for key in &user_config.ssh_authorized_keys {
                let expanded_path = if key.starts_with('~') {
                    if let Some(home) = std::env::var_os("HOME") {
                        std::path::Path::new(&home)
                            .join(key.strip_prefix("~").unwrap().trim_start_matches('/'))
                    } else {
                        std::path::PathBuf::from(key)
                    }
                } else {
                    std::path::PathBuf::from(key)
                };

                let mut key_content = None;
                if let Ok(content) = std::fs::read_to_string(&expanded_path) {
                    key_content = Some(content);
                } else if key.contains("id_rsa.pub") {
                    let fallback_path = expanded_path.with_file_name("id_ed25519.pub");
                    if let Ok(content) = std::fs::read_to_string(fallback_path) {
                        key_content = Some(content);
                    }
                }

                if let Some(content) = key_content {
                    authorized_keys.push(content.trim().to_string());
                } else {
                    authorized_keys.push(key.clone());
                }
            }

            let register = crate::server_interactor::server_interactor_trait::UserRegister::new(
                user_config.name.clone(),
                user_config.groups.clone(),
                authorized_keys,
            );

            node_interactor.create_user(register)?;
        }
    }

    Ok(())
}
