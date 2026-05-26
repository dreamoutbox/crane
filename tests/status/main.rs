use crane::server_interactor::server_interactor_trait::{
    ServerInteractor, ServiceRegister, UserRegister,
};
use crane::ssh::CmdOutput;
use std::cell::RefCell;
use std::path::Path;

struct StatusMockInteractor {
    commands: RefCell<Vec<String>>,
}

impl StatusMockInteractor {
    fn new() -> Self {
        Self {
            commands: RefCell::new(Vec::new()),
        }
    }
}

impl ServerInteractor for StatusMockInteractor {
    fn whoami(&self) -> anyhow::Result<String> {
        Ok("admin".to_string())
    }

    fn cmd(&self, command: &str) -> anyhow::Result<CmdOutput> {
        self.commands.borrow_mut().push(command.to_string());

        let stdout = if command.contains("systemctl list-units") {
            "myapp@3000.service loaded active running crane managed: myapp instance on port 3000\n\
             myapp@3001.service loaded active running crane managed: myapp instance on port 3001"
                .to_string()
        } else if command.contains("/proc/meminfo") {
            // Simulate the combined metrics command output
            "MemTotal:        8192000 kB\n\
             MemAvailable:    6144000 kB\n\
             ===DF===\n\
             Filesystem      Size  Used Avail Use% Mounted on\n\
             /dev/sda1        50G   15G   35G  30% /\n\
             ===METRICS===\n\
             cpu  100 200 300 4000 50 10 5 0 0 0\n\
             ===NET===\n\
             eth0: 1000000 1000 0 0 0 0 0 0 500000 500 0 0 0 0 0 0\n\
             ===SPLIT===\n\
             cpu  110 210 310 4010 55 12 6 0 0 0\n\
             ===NET===\n\
             eth0: 1020480 1020 0 0 0 0 0 0 510240 510 0 0 0 0 0 0\n\
             ===CURLS===\n\
             PORT:3000:STATUS:200\n\
             PORT:3001:STATUS:200"
                .to_string()
        } else {
            "".to_string()
        };

        Ok(CmdOutput {
            stdout,
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
fn test_status_command() {
    let config_path = Path::new("demo/crane.toml");
    let result = crane::commands::status::run(config_path, "myapp", |ssh| {
        // Assert we got correct SSH details for a node
        assert_eq!(ssh.host, "localhost");
        Ok(Box::new(StatusMockInteractor::new()) as Box<dyn ServerInteractor>)
    });

    assert!(result.is_ok());
}
