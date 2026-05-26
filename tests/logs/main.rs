use crane::server_interactor::server_interactor_trait::{
    ServerInteractor, ServiceRegister, UserRegister,
};
use crane::ssh::CmdOutput;
use std::path::Path;
use std::sync::Mutex;

static TEST_MUTEX: Mutex<()> = Mutex::new(());
static LOGS_COMMANDS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn clear_logs_commands() {
    LOGS_COMMANDS.lock().unwrap().clear();
}

struct LogsMockInteractor;

impl LogsMockInteractor {
    fn new() -> Self {
        Self
    }
}

impl ServerInteractor for LogsMockInteractor {
    fn whoami(&self) -> anyhow::Result<String> {
        Ok("admin".to_string())
    }

    fn cmd(&self, command: &str) -> anyhow::Result<CmdOutput> {
        LOGS_COMMANDS.lock().unwrap().push(command.to_string());
        Ok(CmdOutput {
            stdout: "line 1\nline 2".to_string(),
            stderr: "".to_string(),
            exit_code: 0,
        })
    }

    fn get_os_info(&self) -> anyhow::Result<String> {
        Ok("Linux".to_string())
    }

    fn create_file(&self, _path: &str, _content: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn read_file(&self, _path: &str) -> anyhow::Result<String> {
        Ok("".to_string())
    }

    fn upload(&self, _local_path: &str, _remote_path: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn download(&self, _local_path: &str, _remote_path: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn chmod(&self, _path: &str, _permission: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn chown(&self, _path: &str, _user: &str, _group: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn mkdir(&self, _path: &str, _user: &str, _group: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn ls(&self, _path: &str) -> anyhow::Result<Vec<String>> {
        Ok(vec![])
    }

    fn install_dependencies(&self, _dependencies: Vec<String>) -> anyhow::Result<()> {
        Ok(())
    }

    fn register_service(&self, _service_register: ServiceRegister) -> anyhow::Result<()> {
        Ok(())
    }

    fn restart_service(&self, _service_name: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn stop_service(&self, _service_name: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn start_service(&self, _service_name: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn status_service(&self, _service_name: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn delete_service(&self, _service_name: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn create_user(&self, _user_register: UserRegister) -> anyhow::Result<()> {
        Ok(())
    }

    fn delete_user(&self, _username: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_user_to_groups(&self, _username: &str, _groups: Vec<String>) -> anyhow::Result<()> {
        Ok(())
    }

    fn remove_user_from_groups(&self, _username: &str, _groups: Vec<String>) -> anyhow::Result<()> {
        Ok(())
    }

    fn list_users(&self) -> anyhow::Result<Vec<String>> {
        Ok(vec![])
    }
}

#[test]
fn test_logs_command_single_instance() {
    let _guard = TEST_MUTEX.lock().unwrap();
    clear_logs_commands();
    let config_path = Path::new("demo/crane.toml");

    let result = crane::commands::logs::run(
        config_path,
        "myapp@1",
        50,
        Some("1h ago"),
        None,
        true,
        false, // follow
        false, // no_app_instance_id
        |_ssh| {
            let mock = LogsMockInteractor::new();
            Ok(Box::new(mock) as Box<dyn ServerInteractor>)
        },
    );

    assert!(result.is_ok());
    let commands = LOGS_COMMANDS.lock().unwrap().clone();
    assert_eq!(commands.len(), 1);
    assert!(commands[0].contains("journalctl -u myapp@3000.service -n 50 --since '1h ago' --output=short-iso"));
}

#[test]
fn test_logs_command_all_instances() {
    let _guard = TEST_MUTEX.lock().unwrap();
    clear_logs_commands();
    let config_path = Path::new("demo/crane.toml");

    let result = crane::commands::logs::run(
        config_path,
        "myapp",
        100,
        None,
        Some("2026-05-26 12:00:00"),
        false, // timestamps
        false, // follow
        true,  // no_app_instance_id
        |_ssh| {
            let mock = LogsMockInteractor::new();
            Ok(Box::new(mock) as Box<dyn ServerInteractor>)
        },
    );

    assert!(result.is_ok());

    let recorded = LOGS_COMMANDS.lock().unwrap().clone();
    assert_eq!(recorded.len(), 1);
    assert!(recorded.iter().any(|cmd| cmd.contains("journalctl -u myapp@3000.service -n 100 --until '2026-05-26 12:00:00' --output=cat")));
}

#[test]
fn test_logs_command_invalid_instance() {
    let _guard = TEST_MUTEX.lock().unwrap();
    let config_path = Path::new("demo/crane.toml");
    let result = crane::commands::logs::run(
        config_path,
        "myapp@99",
        100,
        None,
        None,
        false,
        false,
        false,
        |_ssh| {
            Ok(Box::new(LogsMockInteractor::new()) as Box<dyn ServerInteractor>)
        },
    );

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("Instance ID 99 is invalid. Total instances: 1"));
}
