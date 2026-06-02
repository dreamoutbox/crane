pub struct SSHSession {
    pub host: String,
    pub username: String,
    pub private_key_path: String,
    pub port: Option<u16>,
}

impl SSHSession {
    pub fn new(
        host: String,
        username: String,
        private_key_path: String,
        port: Option<u16>,
    ) -> Self {
        let debug_ssh_create = std::env::var("DEBUG_SSH");
        if debug_ssh_create.is_ok() {
            // dbg!("SSHSession#new", &host, &username, &port);

            println!(
                "Create new SSH session for {}@{} (port: {})",
                username,
                host,
                port.unwrap_or(22)
            );
        }

        Self {
            host,
            username,
            private_key_path,
            port,
        }
    }

    pub fn run_cmd(&self, cmd: &str) -> anyhow::Result<CmdOutput> {
        let mut command = std::process::Command::new("ssh");
        command.arg("-o").arg("StrictHostKeyChecking=no");
        command.arg("-o").arg("UserKnownHostsFile=/dev/null");

        let control_path = format!("/tmp/crane-{}-{}", self.host, self.port.unwrap_or(22));
        command.arg("-o").arg("ControlMaster=auto");
        command
            .arg("-o")
            .arg(format!("ControlPath={}", control_path));
        command.arg("-o").arg("ControlPersist=60");

        if let Some(port) = self.port {
            command.arg("-p").arg(port.to_string());
        }

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
        let exit_code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Ok(CmdOutput {
            stdout,
            stderr,
            exit_code,
        })
    }

    pub fn spawn_cmd(&self, cmd: &str) -> anyhow::Result<std::process::Child> {
        let mut command = std::process::Command::new("ssh");
        command.arg("-o").arg("StrictHostKeyChecking=no");
        command.arg("-o").arg("UserKnownHostsFile=/dev/null");

        let control_path = format!("/tmp/crane-{}-{}", self.host, self.port.unwrap_or(22));
        command.arg("-o").arg("ControlMaster=auto");
        command
            .arg("-o")
            .arg(format!("ControlPath={}", control_path));
        command.arg("-o").arg("ControlPersist=60");

        if let Some(port) = self.port {
            command.arg("-p").arg(port.to_string());
        }

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

        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::inherit());

        let child = command.spawn()?;
        Ok(child)
    }

    pub fn upload(&self, local_path: &str, remote_path: &str) -> anyhow::Result<()> {
        let mut command = std::process::Command::new("scp");

        command.arg("-o").arg("StrictHostKeyChecking=no");
        command.arg("-o").arg("UserKnownHostsFile=/dev/null");

        let control_path = format!("/tmp/crane-{}-{}", self.host, self.port.unwrap_or(22));
        command.arg("-o").arg("ControlMaster=auto");
        command
            .arg("-o")
            .arg(format!("ControlPath={}", control_path));
        command.arg("-o").arg("ControlPersist=60");

        if let Some(port) = self.port {
            command.arg("-P").arg(port.to_string());
        }

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

        let control_path = format!("/tmp/crane-{}-{}", self.host, self.port.unwrap_or(22));
        command.arg("-o").arg("ControlMaster=auto");
        command
            .arg("-o")
            .arg(format!("ControlPath={}", control_path));
        command.arg("-o").arg("ControlPersist=60");

        if let Some(port) = self.port {
            command.arg("-P").arg(port.to_string());
        }

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

    /// ping is server online?
    pub fn ping(&self) -> bool {
        match self.run_cmd("true") {
            Ok(output) => output.exit_code == 0,
            Err(_) => false,
        }
    }
}

#[derive(Debug)]
pub struct CmdOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}
