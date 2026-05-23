use crate::server_interactor::server_interactor_trait::ServerInteractor;
use super::SSHSession;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct DebianInteractor {
    ssh: SSHSession,
}

impl DebianInteractor {
    pub fn new(ssh: SSHSession) -> Self {
        Self { ssh }
    }
}

impl ServerInteractor for DebianInteractor {
    fn whoami(&self) -> anyhow::Result<String> {
        self.ssh.run_cmd("whoami")
    }

    fn cmd(&self, command: &str) -> anyhow::Result<String> {
        self.ssh.run_cmd(command)
    }

    fn get_os_info(&self) -> anyhow::Result<String> {
        self.ssh.run_cmd("uname -a")
    }

    fn create_file(&self, path: &str, content: &str) -> anyhow::Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let local_path = format!("/tmp/crane-tmp-{}", timestamp);
        std::fs::write(&local_path, content)?;
        
        let upload_res = self.upload(&local_path, path);
        let _ = std::fs::remove_file(&local_path);
        upload_res
    }

    fn read_file(&self, path: &str) -> anyhow::Result<String> {
        self.ssh.run_cmd(&format!("cat '{}'", path))
    }

    fn upload(&self, local_path: &str, remote_path: &str) -> anyhow::Result<()> {
        self.ssh.upload(local_path, remote_path)
    }

    fn download(&self, local_path: &str, remote_path: &str) -> anyhow::Result<()> {
        self.ssh.download(remote_path, local_path)
    }

    fn chmod(&self, path: &str, permission: &str) -> anyhow::Result<()> {
        self.ssh.run_cmd(&format!("chmod '{}' '{}'", permission, path))?;
        Ok(())
    }

    fn chown(&self, path: &str, user: &str, group: &str) -> anyhow::Result<()> {
        self.ssh.run_cmd(&format!("chown '{}:{}' '{}'", user, group, path))?;
        Ok(())
    }
}
