use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub nodes: Vec<NodeConfig>,
    pub users: Option<Vec<UserConfig>>,
    pub app: HashMap<String, AppConfig>,
    pub domain: Option<DomainConfig>,
    pub db: Option<DbConfig>,
    pub backup: Option<BackupConfig>,
    pub monitor: Option<MonitorConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct NodeConfig {
    pub name: String,
    pub host: String,
    pub public_ip: String,
    pub internal_ip: String,
    pub port: u16,
    pub user: String,
    pub roles: Vec<String>,
    pub private_key: String,
    pub sudo_pass: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct UserConfig {
    pub name: String,
    pub groups: Vec<String>,
    pub ssh_authorized_keys: Vec<String>,
    pub private_key: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub name: String,
    pub deploy_dir: String,
    pub entrypoint: String,
    pub pre_deploy_script: Option<String>,
    pub deploy_user: String,
    pub port_start: u16,
    pub port_end: Option<u16>,
    pub instances: u32,
    pub dependencies: Option<Vec<String>>,
    pub health_check_path: Option<String>,
    pub health_check_timeout: Option<u64>,
    pub health_check_interval: Option<u64>,
    pub retain_releases: Option<u32>,
    pub domain: Option<String>,
    pub ssl_cert: Option<String>,
    pub env: Option<HashMap<String, String>>,
    pub min_replicas: Option<u32>,
    pub max_replicas: Option<u32>,
    pub database: Option<Vec<AppDatabaseConfig>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AppDatabaseConfig {
    pub databases: String,
    pub user: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DomainConfig {
    pub provider: String,
    pub domain_name: String,
    pub tls_email: Option<String>,
    pub ssl_cert: Option<String>,
    pub token: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DbConfig {
    pub postgres: Option<PostgresConfig>,
    pub redis: Option<RedisConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PostgresConfig {
    pub enabled: bool,
    pub version: String,
    pub replica_pass: String,
    pub backup: Option<PostgresBackupSchedule>,
    pub users: Option<Vec<PostgresUserConfig>>,
    #[serde(flatten)]
    pub databases: HashMap<String, PostgresDbConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RedisConfig {
    pub enabled: bool,
    pub version: String,
    pub bind: Option<String>,
}

// Postgres database config mapping
#[derive(Debug, Deserialize, Clone)]
pub struct PostgresDbConfig {
    pub name: String,
}

// Postgres user config mapping
#[derive(Debug, Deserialize, Clone)]
pub struct PostgresUserConfig {
    pub user: String,
    pub password: Option<String>,
    pub databases: Vec<String>,
    pub state: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BackupConfig {
    pub s3: Option<S3BackupConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct S3BackupConfig {
    pub bucket: String,
    pub region: Option<String>,
    pub endpoint: Option<String>,
    pub access_key_id: String,
    pub secret_access_key: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PostgresBackupSchedule {
    pub full_backup_every: String,
    pub incremental_backup_every: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MonitorConfig {
    pub interval: Option<u32>,
    pub autoscale: Option<AutoscaleConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AutoscaleConfig {
    pub min_replicas: Option<u32>,
    pub max_replicas: Option<u32>,
    pub scale_up_cpu: Option<u32>,
    pub scale_down_cpu: Option<u32>,
    pub scale_up_memory: Option<u32>,
    pub cooldown: Option<u32>,
}

pub fn read_config_toml_file_with_env<P: AsRef<Path>, E: AsRef<Path>>(
    path: P,
    env_path: Option<E>,
) -> anyhow::Result<Config> {
    let resolved_env_path = env_path
        .map(|p| p.as_ref().to_path_buf())
        .unwrap_or_else(|| Path::new(".env").to_path_buf());

    let dot_env = load_env_file(&resolved_env_path).unwrap_or_default();
    let content = std::fs::read_to_string(path)?;
    let resolved_content = resolve_placeholders(&content, &dot_env)?;
    let config: Config = toml::from_str(&resolved_content)?;

    Ok(config)
}

pub fn read_config_toml_file<P: AsRef<Path>>(path: P) -> anyhow::Result<Config> {
    read_config_toml_file_with_env(path, Option::<&Path>::None)
}

pub fn resolve_placeholders(
    content: &str,
    dot_env: &HashMap<String, String>,
) -> anyhow::Result<String> {
    let mut result = String::new();
    let mut chars = content.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '$' && chars.peek() == Some(&'{') {
            chars.next(); // consume '{'
            let mut var_name = String::new();
            let mut found_close = false;
            while let Some(next_c) = chars.next() {
                if next_c == '}' {
                    found_close = true;
                    break;
                }
                var_name.push(next_c);
            }
            if !found_close {
                anyhow::bail!("Unclosed placeholder: ${{{}", var_name);
            }
            let resolved_value = if let Ok(val) = std::env::var(&var_name) {
                val
            } else if let Some(val) = dot_env.get(&var_name) {
                val.clone()
            } else {
                anyhow::bail!(
                    "Environment variable '{}' not found in either shell environment or env file",
                    var_name
                );
            };
            result.push_str(&resolved_value);
        } else {
            result.push(c);
        }
    }

    Ok(result)
}

// pub fn load_env_into_process<P: AsRef<Path>>(path: P) -> anyhow::Result<()> {
//     let dot_env = load_env_file(path)?;

//     for (k, v) in dot_env {
//         if std::env::var(&k).is_err() {
//             unsafe {
//                 std::env::set_var(&k, v);
//             }
//         }
//     }

//     Ok(())
// }

fn load_env_file<P: AsRef<Path>>(path: P) -> anyhow::Result<HashMap<String, String>> {
    let mut map = HashMap::new();
    if !path.as_ref().exists() {
        return Ok(map);
    }

    let content = std::fs::read_to_string(path)?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some((key, val)) = trimmed.split_once('=') {
            let key = key.trim().to_string();
            let mut val = val.trim().to_string();
            if (val.starts_with('"') && val.ends_with('"'))
                || (val.starts_with('\'') && val.ends_with('\''))
            {
                val.remove(0);
                val.pop();
            }

            map.insert(key, val);
        }
    }

    Ok(map)
}
