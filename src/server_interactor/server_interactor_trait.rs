use std::collections::HashMap;

pub trait ServerInteractor {
    // BASIC
    fn whoami(&self) -> anyhow::Result<String>;
    fn cmd(&self, command: &str) -> anyhow::Result<String>;
    fn get_os_info(&self) -> anyhow::Result<String>;

    // FILES
    fn create_file(&self, path: &str, content: &str) -> anyhow::Result<()>;
    fn read_file(&self, path: &str) -> anyhow::Result<String>;
    fn upload(&self, local_path: &str, remote_path: &str) -> anyhow::Result<()>;
    fn download(&self, local_path: &str, remote_path: &str) -> anyhow::Result<()>;
    fn chmod(&self, path: &str, permission: &str) -> anyhow::Result<()>;
    fn chown(&self, path: &str, user: &str, group: &str) -> anyhow::Result<()>;
    fn mkdir(&self, path: &str, user: &str, group: &str) -> anyhow::Result<()>;
    fn ls(&self, path: &str) -> anyhow::Result<Vec<String>>;

    // DEPENDENCIES
    fn install_dependencies(&self, dependencies: Vec<String>) -> anyhow::Result<()>;

    // SERVICES
    fn register_service(&self, service_register: ServiceRegister) -> anyhow::Result<()>;
    fn restart_service(&self, service_name: &str) -> anyhow::Result<()>;
    fn stop_service(&self, service_name: &str) -> anyhow::Result<()>;
    fn start_service(&self, service_name: &str) -> anyhow::Result<()>;
    fn status_service(&self, service_name: &str) -> anyhow::Result<()>;
    fn delete_service(&self, service_name: &str) -> anyhow::Result<()>;

    // USERS
    fn create_user(&self, user_register: UserRegister) -> anyhow::Result<()>;
    fn delete_user(&self, username: &str) -> anyhow::Result<()>;
    fn add_user_to_groups(&self, username: &str, groups: Vec<String>) -> anyhow::Result<()>;
    fn remove_user_from_groups(&self, username: &str, groups: Vec<String>) -> anyhow::Result<()>;
    fn list_users(&self) -> anyhow::Result<Vec<String>>;
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
