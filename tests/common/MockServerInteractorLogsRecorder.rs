use crane::{
    server_interactor::server_interactor_trait::{ServerInteractor, ServiceRegister, UserRegister},
    ssh::CmdOutput,
};

pub struct MockServerInteractorLogsRecorder;

pub static POSTGRES_LOGS_COMMANDS: std::sync::Mutex<Vec<String>> =
    std::sync::Mutex::new(Vec::new());

impl ServerInteractor for MockServerInteractorLogsRecorder {
    fn whoami(&self) -> anyhow::Result<String> {
        Ok("admin".to_string())
    }
    fn get_os_info(&self) -> anyhow::Result<String> {
        Ok("Linux".to_string())
    }
    fn cmd(&self, command: &str) -> anyhow::Result<CmdOutput> {
        POSTGRES_LOGS_COMMANDS
            .lock()
            .unwrap()
            .push(command.to_string());
        let stdout = if command.contains("pg_current_logfile") {
            "/var/lib/postgresql/17/main/log/postgresql-2026-05-26.csv".to_string()
        } else {
            "dummy log line".to_string()
        };
        Ok(CmdOutput {
            stdout,
            stderr: "".to_string(),
            exit_code: 0,
        })
    }
    fn create_file(&self, _p: &str, _c: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn read_file(&self, _p: &str) -> anyhow::Result<String> {
        Ok("".to_string())
    }
    fn upload(&self, _l: &str, _r: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn download(&self, _l: &str, _r: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn chmod(&self, _p: &str, _perm: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn chown(&self, _p: &str, _u: &str, _g: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn mkdir(&self, _p: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn ls(&self, _p: &str) -> anyhow::Result<Vec<String>> {
        Ok(vec![])
    }
    fn install_dependencies(&self, _d: Vec<String>) -> anyhow::Result<()> {
        Ok(())
    }
    fn register_service(&self, _s: ServiceRegister) -> anyhow::Result<()> {
        Ok(())
    }
    fn restart_service(&self, _s: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn stop_service(&self, _s: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn start_service(&self, _s: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn status_service(&self, _s: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn delete_service(&self, _s: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn create_user(&self, _u: UserRegister) -> anyhow::Result<()> {
        Ok(())
    }
    fn delete_user(&self, _u: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn add_user_to_groups(&self, _u: &str, _g: Vec<String>) -> anyhow::Result<()> {
        Ok(())
    }
    fn remove_user_from_groups(&self, _u: &str, _g: Vec<String>) -> anyhow::Result<()> {
        Ok(())
    }
    fn list_users(&self) -> anyhow::Result<Vec<String>> {
        Ok(vec![])
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

    fn unzip(&self, _: &str, _: &str) -> Result<(), anyhow::Error> {
        Ok(())
    }

    fn spawn_cmd(&self, _: &str) -> anyhow::Result<std::process::Child> {
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
    fn setup_firewall(&self, _config: &crane::config::Config) -> anyhow::Result<()> {
        let _ = self.cmd("setup_firewall")?;
        Ok(())
    }

    fn mv(&self, _src: &str, _dest: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn cp(&self, _src: &str, _dest: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn exists(&self, _path: &str) -> anyhow::Result<bool> {
        Ok(false)
    }

    fn rm(&self, _path: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn tar_extract(&self, _archive: &str, _dest: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn user_exists(&self, username: &str) -> anyhow::Result<bool> {
        let _ = self.cmd(&format!("id {}", username))?;
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
