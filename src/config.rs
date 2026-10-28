use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct PortRange {
    pub start: u16,
    pub end: u16,
}

impl TryFrom<String> for PortRange {
    type Error = anyhow::Error;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        let s = s.trim();
        if let Some((start_s, end_s)) = s.split_once('-') {
            let start: u16 = start_s
                .trim()
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid start port in range '{}'", s))?;
            let end: u16 = end_s
                .trim()
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid end port in range '{}'", s))?;
            if start == 0 || end == 0 {
                anyhow::bail!("Port cannot be 0 in range '{}'", s);
            }
            if start > end {
                anyhow::bail!(
                    "Start port cannot be greater than end port in range '{}'",
                    s
                );
            }
            Ok(PortRange { start, end })
        } else {
            let port: u16 = s
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid port '{}'", s))?;
            if port == 0 {
                anyhow::bail!("Port cannot be 0 in '{}'", s);
            }
            Ok(PortRange {
                start: port,
                end: port,
            })
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub nodes: Vec<NodeConfig>,
    pub users: Option<Vec<UserConfig>>,
    pub app: HashMap<String, AppConfig>,
    pub domain: Option<DomainConfig>,
    pub db: Option<DbConfig>,
    pub backup: Option<BackupConfig>,
    // pub monitor: Option<MonitorConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct NodeConfig {
    pub name: String,
    pub public_ip: String,
    pub internal_ip: String,
    pub ssh_user: String,
    pub roles: Vec<String>,
    pub ssh_ip: String,
    pub ssh_port: u16,
    pub private_key: String,
    pub sudo_pass: Option<String>,
    pub firewall_expose_port_range: Option<Vec<PortRange>>,
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
    pub dependencies: Option<Vec<String>>,
    pub health_check_path: Option<String>,
    pub health_check_timeout: Option<u64>,
    pub health_check_interval: Option<u64>,
    // pub retain_releases: Option<u32>,
    pub domain: Option<String>,
    pub ssl_cert: Option<String>,
    pub env: Option<HashMap<String, String>>,
    pub instances: u32,
    // pub min_replicas: Option<u32>,
    // pub max_replicas: Option<u32>,
    pub database: Option<Vec<AppDatabaseConfig>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AppDatabaseConfig {
    pub databases: String,
    pub user: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DomainConfig {
    pub enabled: bool,
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
    #[serde(default)]
    pub minimal: bool,
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
    pub enabled: bool,
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

/// Checks whether a path configured in the TOML exists relative to the config file directory or CWD.
///
/// # Arguments
/// * `raw_path` - The path string specified in the configuration file
/// * `config_dir` - The directory containing the configuration file
///
/// # Returns
/// `true` if the path exists at either location, otherwise `false`.
///
/// # Example
/// ```no_run
/// use std::path::Path;
/// use crane::config::check_config_path_exists;
///
/// let exists = check_config_path_exists("keys/id_ed25519", Path::new("."));
/// ```
pub fn check_config_path_exists<P: AsRef<Path>>(raw_path: &str, config_dir: P) -> bool {
    let raw = Path::new(raw_path);
    if raw.is_absolute() {
        return raw.exists();
    }
    config_dir.as_ref().join(raw).exists() || raw.exists()
}

/// Validates that referenced local filesystem paths in configuration exist.
///
/// Checks node private keys, user private keys and authorized key files,
/// and app deploy dirs, entrypoints, and pre-deploy scripts.
///
/// # Arguments
/// * `config` - Reference to loaded configuration
/// * `config_file_path` - Path to configuration file used to resolve relative paths
///
/// # Returns
/// `Ok(())` if all referenced paths exist, or `Err` with descriptive message naming missing paths.
pub fn validate_config_paths<P: AsRef<Path>>(
    config: &Config,
    config_file_path: P,
) -> anyhow::Result<()> {
    let config_dir = config_file_path
        .as_ref()
        .parent()
        .unwrap_or_else(|| Path::new("."));

    // Check node private keys
    for node in &config.nodes {
        if !check_config_path_exists(&node.private_key, config_dir) {
            anyhow::bail!(
                "Node '{}' private_key not found at '{}'",
                node.name,
                node.private_key
            );
        }
    }

    // Check user private keys and ssh authorized keys
    if let Some(ref users) = config.users {
        for user in users {
            if !check_config_path_exists(&user.private_key, config_dir) {
                anyhow::bail!(
                    "User '{}' private_key not found at '{}'",
                    user.name,
                    user.private_key
                );
            }
            for key_path in &user.ssh_authorized_keys {
                // If it looks like a public key file path, check if it exists
                if (key_path.ends_with(".pub") || key_path.contains('/'))
                    && !check_config_path_exists(key_path, config_dir)
                {
                    anyhow::bail!(
                        "User '{}' ssh_authorized_key file not found at '{}'",
                        user.name,
                        key_path
                    );
                }
            }
        }
    }

    // Check app paths: deploy_dir, entrypoint, pre_deploy_script
    for (app_key, app) in &config.app {
        if !check_config_path_exists(&app.deploy_dir, config_dir) {
            anyhow::bail!(
                "App '{}' deploy_dir not found at '{}'",
                app_key,
                app.deploy_dir
            );
        }

        let resolved_deploy_dir = if config_dir.join(&app.deploy_dir).exists() {
            config_dir.join(&app.deploy_dir)
        } else {
            Path::new(&app.deploy_dir).to_path_buf()
        };

        // Check entrypoint inside deploy_dir
        let entrypoint_subpath = app.entrypoint.trim_start_matches("./");
        let entrypoint_full = resolved_deploy_dir.join(entrypoint_subpath);
        if !entrypoint_full.exists() && !check_config_path_exists(&app.entrypoint, config_dir) {
            anyhow::bail!(
                "App '{}' entrypoint not found at '{}'",
                app_key,
                app.entrypoint
            );
        }

        // Check pre_deploy_script if specified
        if let Some(ref pre_script) = app.pre_deploy_script {
            let pre_script_subpath = pre_script.trim_start_matches("./");
            let pre_script_full = resolved_deploy_dir.join(pre_script_subpath);
            if !pre_script_full.exists() && !check_config_path_exists(pre_script, config_dir) {
                anyhow::bail!(
                    "App '{}' pre_deploy_script not found at '{}'",
                    app_key,
                    pre_script
                );
            }
        }
    }

    Ok(())
}

/// Validates configuration rules and topology constraints.
///
/// Ensures that when `[db.postgres]` is enabled in minimal mode (`minimal = true`),
/// exactly one node in `config.nodes` has the `postgres` role.
///
/// # Arguments
/// * `config` - Reference to loaded configuration
///
/// # Returns
/// `Ok(())` if configuration passes validation, or `Err` with descriptive message.
///
/// # Example
/// ```no_run
/// use crane::config::{Config, validate_config};
///
/// # fn test(cfg: &Config) -> anyhow::Result<()> {
/// validate_config(cfg)?;
/// # Ok(())
/// # }
/// ```
pub fn validate_config(config: &Config) -> anyhow::Result<()> {
    if postgres_is_minimal(config) {
        let pg_nodes_count = config
            .nodes
            .iter()
            .filter(|n| n.roles.iter().any(|r| r == "postgres"))
            .count();
        if pg_nodes_count != 1 {
            anyhow::bail!(
                "[db.postgres] minimal mode requires exactly 1 node with role 'postgres', but found {}",
                pg_nodes_count
            );
        }
    }
    Ok(())
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
    validate_config(&config)?;

    // Initialize/update the global server interactor registry with the loaded config
    crate::server_interactor::init_registry(config.clone());

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

pub fn find_node_config<'a>(config: &'a Config, target: &str) -> Option<&'a NodeConfig> {
    config
        .nodes
        .iter()
        .find(|n| n.internal_ip == target || n.public_ip == target || n.name == target)
}

pub fn get_pg_replica_pass(config: &Config) -> String {
    config
        .db
        .as_ref()
        .and_then(|db| db.postgres.as_ref())
        .map(|pg| pg.replica_pass.clone())
        .unwrap_or_else(|| "replica".to_string())
}

// Parses databases and users configs from config
pub fn get_postgres_dbs_and_users_config(
    config: &crate::config::Config,
) -> (Vec<PostgresDbConfig>, Vec<PostgresUserConfig>) {
    let mut db_configs = Vec::new();
    let mut user_configs = std::collections::HashMap::new();

    if let Some(ref db) = config.db {
        if let Some(ref pg) = db.postgres {
            // 1. Parse databases
            for (_key, val) in &pg.databases {
                db_configs.push(PostgresDbConfig {
                    name: val.name.clone(),
                });
            }

            // 2. config postgres users
            if let Some(ref users_list) = pg.users {
                for u in users_list {
                    let user_entry =
                        user_configs
                            .entry(u.user.clone())
                            .or_insert_with(|| PostgresUserConfig {
                                user: u.user.clone(),
                                password: u.password.clone(),
                                databases: Vec::new(),
                                state: u.state.clone(),
                            });

                    for db_name in &u.databases {
                        if !user_entry.databases.contains(db_name) {
                            user_entry.databases.push(db_name.clone());
                        }
                    }
                }
            }
        }
    }

    let users = user_configs.into_values().collect();

    (db_configs, users)
}

pub fn get_postgres_backup_schedule_config(
    config: &crate::config::Config,
) -> Option<PostgresBackupSchedule> {
    config
        .db
        .as_ref()
        .and_then(|db| db.postgres.as_ref())
        .and_then(|pg| pg.backup.clone())
}

/// Returns `[domain]` only when it is present and `enabled = true`.
pub fn enabled_domain(config: &Config) -> Option<&DomainConfig> {
    config.domain.as_ref().filter(|d| d.enabled)
}

/// Returns the app's configured domain only if `[domain]` is present and enabled.
pub fn app_domain<'a>(config: &'a Config, app: &'a AppConfig) -> Option<&'a str> {
    if enabled_domain(config).is_some() {
        app.domain.as_deref()
    } else {
        None
    }
}

/// Returns the app's configured ssl_cert only if `[domain]` is present and enabled.
pub fn app_ssl_cert<'a>(config: &'a Config, app: &'a AppConfig) -> Option<&'a str> {
    if enabled_domain(config).is_some() {
        app.ssl_cert.as_deref()
    } else {
        None
    }
}

/// Returns true when `[backup.s3]` is present and `enabled = true`.
pub fn s3_backup_enabled(config: &Config) -> bool {
    config
        .backup
        .as_ref()
        .and_then(|b| b.s3.as_ref())
        .map(|s| s.enabled)
        .unwrap_or(false)
}

/// Checks whether PostgreSQL minimal mode is active.
///
/// When minimal mode is enabled, PostgreSQL runs as a standalone single-node service
/// without Patroni or etcd high-availability clustering.
///
/// # Arguments
/// * `config` - Reference to loaded configuration
///
/// # Returns
/// `true` if `[db.postgres]` is configured, `enabled = true`, and `minimal = true`, otherwise `false`.
///
/// # Example
/// ```no_run
/// use crane::config::{Config, postgres_is_minimal};
///
/// # fn check(cfg: &Config) {
/// if postgres_is_minimal(cfg) {
///     println!("Postgres minimal mode active");
/// }
/// # }
/// ```
pub fn postgres_is_minimal(config: &Config) -> bool {
    config
        .db
        .as_ref()
        .and_then(|db| db.postgres.as_ref())
        .map(|pg| pg.enabled && pg.minimal)
        .unwrap_or(false)
}
