pub mod debian;
pub mod server_interactor_trait;

pub struct SSHSession {
    pub host: String,
    pub username: String,
    pub private_key_path: String,
}

impl SSHSession {
    pub fn new(host: String, username: String, private_key_path: String) -> Self {
        Self {
            host,
            username,
            private_key_path,
        }
    }

    pub fn run_cmd(&self, cmd: &str) -> anyhow::Result<String> {
        let mut command = std::process::Command::new("ssh");
        command.arg("-o").arg("StrictHostKeyChecking=no");
        command.arg("-o").arg("UserKnownHostsFile=/dev/null");

        if !self.private_key_path.is_empty() {
            command.arg("-i").arg(&self.private_key_path);
        }

        let destination = if !self.username.is_empty() {
            format!("{}@{}", self.username, self.host)
        } else {
            self.host.clone()
        };
        command.arg(destination);
        command.arg(cmd);

        let output = command.output()?;
        if !output.status.success() {
            let err_msg = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("SSH command failed: {}", err_msg.trim()));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    pub fn upload(&self, local_path: &str, remote_path: &str) -> anyhow::Result<()> {
        let mut command = std::process::Command::new("scp");
        command.arg("-o").arg("StrictHostKeyChecking=no");
        command.arg("-o").arg("UserKnownHostsFile=/dev/null");

        if !self.private_key_path.is_empty() {
            command.arg("-i").arg(&self.private_key_path);
        }

        command.arg(local_path);

        let destination = if !self.username.is_empty() {
            format!("{}@{}:{}", self.username, self.host, remote_path)
        } else {
            format!("{}:{}", self.host, remote_path)
        };
        command.arg(destination);

        let output = command.output()?;
        if !output.status.success() {
            let err_msg = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("SCP upload failed: {}", err_msg.trim()));
        }
        Ok(())
    }

    pub fn download(&self, remote_path: &str, local_path: &str) -> anyhow::Result<()> {
        let mut command = std::process::Command::new("scp");
        command.arg("-o").arg("StrictHostKeyChecking=no");
        command.arg("-o").arg("UserKnownHostsFile=/dev/null");

        if !self.private_key_path.is_empty() {
            command.arg("-i").arg(&self.private_key_path);
        }

        let source = if !self.username.is_empty() {
            format!("{}@{}:{}", self.username, self.host, remote_path)
        } else {
            format!("{}:{}", self.host, remote_path)
        };
        command.arg(source);
        command.arg(local_path);

        let output = command.output()?;
        if !output.status.success() {
            let err_msg = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("SCP download failed: {}", err_msg.trim()));
        }
        Ok(())
    }
}
