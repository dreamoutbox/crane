use crane::{
    server_interactor::server_interactor_trait::{ServerInteractor, ServiceRegister, UserRegister},
    ssh::CmdOutput,
};

pub struct MockServerInteractorUserNotExist;

impl ServerInteractor for MockServerInteractorUserNotExist {
    fn whoami(&self) -> anyhow::Result<String> {
        Ok("admin".to_string())
    }
    fn cmd(&self, command: &str) -> anyhow::Result<CmdOutput> {
        if command == "id postgres" {
            Ok(CmdOutput {
                stdout: "".to_string(),
                stderr: "id: 'postgres': no such user".to_string(),
                exit_code: 1,
            })
        } else {
            Ok(CmdOutput {
                stdout: "".to_string(),
                stderr: "".to_string(),
                exit_code: 0,
            })
        }
    }
    fn get_os_info(&self) -> anyhow::Result<String> {
        Ok("Linux".to_string())
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
}
