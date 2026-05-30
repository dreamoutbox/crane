struct MockInteractor {
    commands: RefCell<Vec<String>>,
    files: RefCell<HashMap<String, String>>,
    simulated_dates: Vec<String>,
}

impl MockInteractor {
    fn new(simulated_dates: Vec<String>) -> Self {
        Self {
            commands: RefCell::new(Vec::new()),
            files: RefCell::new(HashMap::new()),
            simulated_dates,
        }
    }
}

impl ServerInteractor for MockInteractor {
    fn whoami(&self) -> anyhow::Result<String> {
        Ok("postgres".to_string())
    }
    fn cmd(&self, command: &str) -> anyhow::Result<CmdOutput> {
        self.commands.borrow_mut().push(command.to_string());
        let stdout = if command.contains("date") {
            let count = self
                .commands
                .borrow()
                .iter()
                .filter(|c| c.contains("date"))
                .count();
            self.simulated_dates
                .get(count - 1)
                .cloned()
                .unwrap_or_else(|| "20251211152749155 2025-12-11 15:27:49".to_string())
        } else if command.contains("pg_is_in_recovery") {
            "f".to_string()
        } else if command.contains("/primary") {
            "200".to_string()
        } else if command.contains("patronictl") && command.contains("list") {
            "vps1 running".to_string()
        } else if command.contains("lsb_release") {
            "distro=debian".to_string()
        } else if command.contains("test -f") {
            if command.contains("registry.toml") {
                "yes".to_string()
            } else {
                "no".to_string()
            }
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
    fn create_file(&self, path: &str, content: &str) -> anyhow::Result<()> {
        self.files
            .borrow_mut()
            .insert(path.to_string(), content.to_string());
        Ok(())
    }
    fn read_file(&self, path: &str) -> anyhow::Result<String> {
        self.files
            .borrow_mut()
            .get(path)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("File not found"))
    }
    fn upload(&self, _local_path: &str, _remote_path: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn download(&self, local_path: &str, _remote_path: &str) -> anyhow::Result<()> {
        // Write a dummy file to local_path so std::fs::read works in testing
        std::fs::write(local_path, b"dummy data")?;
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
        Ok(vec!["base.tar".to_string(), "backup_manifest".to_string()])
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
