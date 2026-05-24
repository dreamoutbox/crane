use crate::config;

pub fn find_private_key_for_user(username: &str, config: &config::Config) -> String {
    if let Some(ref users) = config.users {
        if let Some(user_config) = users.iter().find(|u| u.name == username) {
            for key in &user_config.ssh_authorized_keys {
                let expanded = if key.starts_with('~') {
                    if let Some(home) = std::env::var_os("HOME") {
                        std::path::Path::new(&home)
                            .join(key.strip_prefix("~").unwrap().trim_start_matches('/'))
                    } else {
                        std::path::PathBuf::from(key)
                    }
                } else {
                    std::path::PathBuf::from(key)
                };

                let candidate = if expanded.extension().map_or(false, |ext| ext == "pub") {
                    expanded.with_extension("")
                } else {
                    expanded.clone()
                };

                if candidate.exists() {
                    return candidate.to_string_lossy().to_string();
                }

                if key.contains("id_rsa.pub") {
                    let fallback = candidate.with_file_name("id_ed25519");
                    if fallback.exists() {
                        return fallback.to_string_lossy().to_string();
                    }
                }
            }
        }
    }

    "".to_string()
}

pub fn get_any_private_key(config: &config::Config) -> String {
    if let Some(ref users) = config.users {
        for user_config in users {
            for key in &user_config.ssh_authorized_keys {
                let expanded = if key.starts_with('~') {
                    if let Some(home) = std::env::var_os("HOME") {
                        std::path::Path::new(&home)
                            .join(key.strip_prefix("~").unwrap().trim_start_matches('/'))
                    } else {
                        std::path::PathBuf::from(key)
                    }
                } else {
                    std::path::PathBuf::from(key)
                };

                let candidate = if expanded.extension().map_or(false, |ext| ext == "pub") {
                    expanded.with_extension("")
                } else {
                    expanded.clone()
                };

                if candidate.exists() {
                    return candidate.to_string_lossy().to_string();
                }

                if key.contains("id_rsa.pub") {
                    let fallback = candidate.with_file_name("id_ed25519");
                    if fallback.exists() {
                        return fallback.to_string_lossy().to_string();
                    }
                }
            }
        }
    }

    "".to_string()
}
