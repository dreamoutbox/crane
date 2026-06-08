use std::collections::HashMap;

use crate::config;
use crate::ssh::CmdOutput;

pub trait ServerInteractor {
    // BASIC
    fn whoami(&self) -> anyhow::Result<String>;
    fn cmd(&self, command: &str) -> anyhow::Result<CmdOutput>;
    fn spawn_cmd(&self, command: &str) -> anyhow::Result<std::process::Child>;
    fn get_os_info(&self) -> anyhow::Result<String>;

    // FILES
    fn create_file(&self, path: &str, content: &str) -> anyhow::Result<()>;
    fn read_file(&self, path: &str) -> anyhow::Result<String>;
    fn upload(&self, local_path: &str, remote_path: &str) -> anyhow::Result<()>;
    fn download(&self, local_path: &str, remote_path: &str) -> anyhow::Result<()>;
    fn chmod(&self, path: &str, permission: &str) -> anyhow::Result<()>;
    fn chown(&self, path: &str, user: &str, group: &str) -> anyhow::Result<()>;
    fn mkdir(&self, path: &str) -> anyhow::Result<()>;
    fn ls(&self, path: &str) -> anyhow::Result<Vec<String>>;
    fn unzip(&self, zip_path: &str, dest: &str) -> anyhow::Result<()>;
    fn mv(&self, src: &str, dest: &str) -> anyhow::Result<()>;
    fn cp(&self, src: &str, dest: &str) -> anyhow::Result<()>;
    fn exists(&self, path: &str) -> anyhow::Result<bool>;
    fn rm(&self, path: &str) -> anyhow::Result<()>;
    fn tar_extract(&self, archive: &str, dest: &str) -> anyhow::Result<()>;

    // DEPENDENCIES
    fn install_dependencies(&self, dependencies: Vec<String>) -> anyhow::Result<()>;

    // SERVICES
    fn register_service(&self, service_register: ServiceRegister) -> anyhow::Result<()>;
    fn restart_service(&self, service_name: &str) -> anyhow::Result<()>;
    fn stop_service(&self, service_name: &str) -> anyhow::Result<()>;
    fn start_service(&self, service_name: &str) -> anyhow::Result<()>;
    fn status_service(&self, service_name: &str) -> anyhow::Result<()>;
    fn delete_service(&self, service_name: &str) -> anyhow::Result<()>;
    fn wait_for_service_start(&self, service_name: &str, timeout: u64) -> anyhow::Result<bool>;
    fn service_daemon_reload(&self) -> anyhow::Result<()>;
    fn enable_service(&self, service_name: &str) -> anyhow::Result<()>;
    fn disable_service(&self, service_name: &str) -> anyhow::Result<()>;
    fn wait_for_service_status(
        &self,
        service_name: &str,
        service_status: &str,
        timeout: u64,
    ) -> anyhow::Result<bool>;

    // USERS
    fn create_user(&self, user_register: UserRegister) -> anyhow::Result<()>;
    fn delete_user(&self, username: &str) -> anyhow::Result<()>;
    fn add_user_to_groups(&self, username: &str, groups: Vec<String>) -> anyhow::Result<()>;
    fn remove_user_from_groups(&self, username: &str, groups: Vec<String>) -> anyhow::Result<()>;
    fn list_users(&self) -> anyhow::Result<Vec<String>>;

    // FIREWALL
    fn firewall_enable(&self, enable: bool) -> anyhow::Result<()>;
    fn firewall_reload(&self) -> anyhow::Result<()>;
    fn firewall_reset(&self) -> anyhow::Result<()>;
    fn firewall_allow_port(
        &self,
        port: u16,
        proto: &str,
        source: Option<&str>,
    ) -> anyhow::Result<()>;
    fn firewall_deny_port(
        &self,
        port: u16,
        proto: &str,
        source: Option<&str>,
    ) -> anyhow::Result<()>;
    fn firewall_allow_source(&self, source: &str) -> anyhow::Result<()>;

    // UTILITIES & HELPER METHODS
    fn user_exists(&self, username: &str) -> anyhow::Result<bool>;
    fn which(&self, binary: &str) -> anyhow::Result<String>;
    fn check_http_status(&self, url: &str) -> anyhow::Result<u16>;
    fn update_etc_hosts(&self, entries: &[(String, String)]) -> anyhow::Result<()>;
    fn generate_self_signed_cert(
        &self,
        key_path: &str,
        crt_path: &str,
        cert_path: &str,
    ) -> anyhow::Result<()>;

    fn kill_postgres_processes(&self) -> anyhow::Result<()>;

    fn psql(
        &self,
        command: Option<&str>,
        file: Option<&str>,
        dbname: Option<&str>,
        tuples_only: bool,
    ) -> anyhow::Result<CmdOutput>;

    // postgres & patroni
    fn install_postgres(&self, version: &str) -> anyhow::Result<()>;
    fn setup_patroni(
        &self,
        node: &config::NodeConfig,
        pg_version: &String,
        replica_pass: &String,
        pg_nodes: &Vec<config::NodeConfig>,
    ) -> anyhow::Result<bool>;

    //etcd
    fn setup_etcd(
        &self,
        node: &config::NodeConfig,
        pg_nodes: &[config::NodeConfig],
    ) -> anyhow::Result<()>;
    fn start_etcd(&self, node: &config::NodeConfig) -> anyhow::Result<()>;
}

pub struct ServiceRegister {
    pub service_name: String,
    pub binary_path: String,
    pub default_port: u16,
    pub args: Vec<String>,
    pub working_directory: String,
    pub user: String,
    pub auto_restart: bool,
    pub auto_start: bool,
    pub environment_variables: HashMap<String, String>,
    pub resource_limit: ResourceLimit,
}

impl ServiceRegister {
    pub fn new(
        service_name: String,
        binary_path: String,
        default_port: u16,
        args: Vec<String>,
        working_directory: String,
        user: String,
        auto_restart: bool,
        auto_start: bool,
        environment_variables: HashMap<String, String>,
        resource_limit: ResourceLimit,
    ) -> Self {
        Self {
            service_name,
            binary_path,
            default_port,
            args,
            working_directory,
            user,
            auto_restart,
            auto_start,
            environment_variables,
            resource_limit,
        }
    }
}

pub struct ResourceLimit {
    pub cpu_cores: u32,
    pub memory_limit_mb: u64,
}

impl ResourceLimit {
    pub fn new(cpu_cores: u32, memory_limit_mb: u64) -> Self {
        Self {
            cpu_cores,
            memory_limit_mb,
        }
    }
}

pub struct UserRegister {
    pub username: String,
    pub groups: Vec<String>,
    pub ssh_authorized_keys: Vec<String>,
}

impl UserRegister {
    pub fn new(username: String, groups: Vec<String>, ssh_authorized_keys: Vec<String>) -> Self {
        Self {
            username,
            groups,
            ssh_authorized_keys,
        }
    }
}
