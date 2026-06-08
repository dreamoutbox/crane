use crane::server_interactor::server_interactor_trait::{
    ServerInteractor, ServiceRegister, UserRegister,
};
use crane::ssh::CmdOutput;
use std::cell::RefCell;
use std::collections::HashMap;
use std::process::Child;

#[allow(unused)]

pub struct MockInteractor {
    pub commands: RefCell<Vec<String>>,
    pub files: RefCell<HashMap<String, String>>,
    pub simulated_dates: Vec<String>,
}

impl MockInteractor {
    #[allow(dead_code)]
    pub fn new(simulated_dates: Vec<String>) -> Self {
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
    fn mkdir(&self, _path: &str) -> anyhow::Result<()> {
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

    fn unzip(&self, _: &str, _: &str) -> Result<(), anyhow::Error> {
        Ok(())
    }

    fn wait_for_service_start(&self, _: &str, _: u64) -> Result<bool, anyhow::Error> {
        Ok(true)
    }

    fn service_daemon_reload(&self) -> Result<(), anyhow::Error> {
        Ok(())
    }
    fn enable_service(&self, _: &str) -> Result<(), anyhow::Error> {
        Ok(())
    }
    fn disable_service(&self, _: &str) -> Result<(), anyhow::Error> {
        Ok(())
    }
    fn spawn_cmd(&self, _: &str) -> Result<Child, anyhow::Error> {
        todo!()
    }
    fn firewall_enable(&self, _enable: bool) -> anyhow::Result<()> {
        Ok(())
    }
    fn firewall_reload(&self) -> anyhow::Result<()> {
        Ok(())
    }
    fn firewall_reset(&self) -> anyhow::Result<()> {
        Ok(())
    }
    fn firewall_allow_port(
        &self,
        _port: u16,
        _proto: &str,
        _source: Option<&str>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
    fn firewall_deny_port(
        &self,
        _port: u16,
        _proto: &str,
        _source: Option<&str>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
    fn firewall_allow_source(&self, _source: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn mv(&self, src: &str, dest: &str) -> anyhow::Result<()> {
        self.commands
            .borrow_mut()
            .push(format!("sudo mv '{}' '{}'", src, dest));
        Ok(())
    }

    fn cp(&self, src: &str, dest: &str) -> anyhow::Result<()> {
        self.commands
            .borrow_mut()
            .push(format!("sudo cp -r '{}' '{}'", src, dest));
        Ok(())
    }

    fn exists(&self, path: &str) -> anyhow::Result<bool> {
        self.commands
            .borrow_mut()
            .push(format!("sudo test -e '{}'", path));
        if path.contains("registry.toml") {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn rm(&self, path: &str) -> anyhow::Result<()> {
        self.commands
            .borrow_mut()
            .push(format!("sudo rm -rf '{}'", path));
        Ok(())
    }

    fn tar_extract(&self, archive: &str, dest: &str) -> anyhow::Result<()> {
        self.commands
            .borrow_mut()
            .push(format!("sudo tar -xf '{}' -C '{}'", archive, dest));
        Ok(())
    }

    fn user_exists(&self, username: &str) -> anyhow::Result<bool> {
        let _ = self.cmd(&format!("id -u {}", username))?;
        Ok(true)
    }

    fn which(&self, binary: &str) -> anyhow::Result<String> {
        let out = self.cmd(&format!("which {}", binary))?;
        if out.exit_code == 0 && !out.stdout.trim().is_empty() {
            Ok(out.stdout.trim().to_string())
        } else {
            anyhow::bail!("{} not found", binary)
        }
    }

    fn check_http_status(&self, url: &str) -> anyhow::Result<u16> {
        let out = self.cmd(&format!("curl -s -o /dev/null -w \"HTTP_CODE\" {}", url))?;
        if out.stdout.trim() == "200" {
            Ok(200)
        } else {
            Ok(0)
        }
    }

    fn update_etc_hosts(&self, entries: &[(String, String)]) -> anyhow::Result<()> {
        let _ = self.cmd(&format!("update_etc_hosts {:?}", entries))?;
        Ok(())
    }

    fn generate_self_signed_cert(
        &self,
        key_path: &str,
        crt_path: &str,
        cert_path: &str,
    ) -> anyhow::Result<()> {
        let _ = self.cmd(&format!(
            "generate_self_signed_cert {} {} {}",
            key_path, crt_path, cert_path
        ))?;
        Ok(())
    }

    fn wait_for_service_status(
        &self,
        service_name: &str,
        service_status: &str,
        timeout: u64,
    ) -> anyhow::Result<bool> {
        let _ = self.cmd(&format!(
            "wait_for_service_status {} {} {}",
            service_name, service_status, timeout
        ))?;
        Ok(true)
    }

    fn install_postgres(&self, version: &str) -> anyhow::Result<()> {
        let _ = self.cmd(&format!("install_postgres {}", version))?;
        Ok(())
    }

    fn kill_postgres_processes(&self) -> anyhow::Result<()> {
        let _ = self.cmd("sudo pkill -9 -u postgres postgres")?;
        Ok(())
    }

    fn psql(
        &self,
        command: Option<&str>,
        file: Option<&str>,
        dbname: Option<&str>,
        tuples_only: bool,
    ) -> anyhow::Result<CmdOutput> {
        let mut cmd = "sudo -u postgres psql".to_string();
        if tuples_only {
            cmd.push_str(" -t -A");
        }
        if let Some(db) = dbname {
            cmd.push_str(&format!(" -d {}", db));
        }
        if let Some(c) = command {
            cmd.push_str(&format!(" -c \"{}\"", c));
        } else if let Some(f) = file {
            cmd.push_str(&format!(" -f '{}'", f));
        }
        self.cmd(&cmd)
    }

    fn setup_patroni(
        &self,
        _node: &crane::config::NodeConfig,
        _pg_version: &String,
        _replica_pass: &String,
        _pg_nodes: &Vec<crane::config::NodeConfig>,
    ) -> anyhow::Result<bool> {
        todo!()
    }

    fn setup_etcd(
        &self,
        _node: &crane::config::NodeConfig,
        _pg_nodes: &[crane::config::NodeConfig],
    ) -> anyhow::Result<()> {
        todo!()
    }

    fn start_etcd(&self, _node: &crane::config::NodeConfig) -> anyhow::Result<()> {
        todo!()
    }
}
