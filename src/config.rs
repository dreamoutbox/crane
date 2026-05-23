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
    pub host: String,
    pub public_ip: String,
    pub internal_ip: String,
    pub port: u16,
    pub user: String,
    pub roles: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct UserConfig {
    pub name: String,
    pub groups: Vec<String>,
    pub ssh_authorized_keys: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub name: String,
    pub binary: String,
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
    pub env: Option<HashMap<String, String>>,
    pub min_replicas: Option<u32>,
    pub max_replicas: Option<u32>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DomainConfig {
    pub provider: String,
    pub name: String,
    pub tls_email: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DbConfig {
    pub postgres: Option<HashMap<String, toml::Value>>,
    pub redis: Option<HashMap<String, toml::Value>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BackupConfig {
    pub s3: Option<HashMap<String, toml::Value>>,
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

pub fn load_config<P: AsRef<Path>>(path: P) -> anyhow::Result<Config> {
    let content = std::fs::read_to_string(path)?;
    let config: Config = toml::from_str(&content)?;
    Ok(config)
}

pub fn load_env_file<P: AsRef<Path>>(path: P) -> anyhow::Result<HashMap<String, String>> {
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
