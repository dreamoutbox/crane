use super::SSHSession;
use crate::server_interactor::server_interactor_trait::ServerInteractor;
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
        self.ssh
            .run_cmd(&format!("chmod '{}' '{}'", permission, path))?;
        Ok(())
    }

    fn chown(&self, path: &str, user: &str, group: &str) -> anyhow::Result<()> {
        self.ssh
            .run_cmd(&format!("chown '{}:{}' '{}'", user, group, path))?;
        Ok(())
    }

    fn register_service(
        &self,
        service_register: super::server_interactor_trait::ServiceRegister,
    ) -> anyhow::Result<()> {
        let args_str = service_register.args.join(" ");
        let mut env_str = String::new();
        for (k, v) in &service_register.environment_variables {
            env_str.push_str(&format!("Environment=\"{}={}\"\n", k, v));
        }

        let restart_str = if service_register.auto_restart {
            "Restart=on-failure\nRestartSec=5"
        } else {
            "Restart=no"
        };

        let service_content = format!(
            "[Unit]\nDescription=crane managed: {name} service\nAfter=network.target\n\n[Service]\nType=simple\nUser={user}\nWorkingDirectory={work_dir}\nExecStart={bin_path} {args}\n{env}{restart}\n\n[Install]\nWantedBy=multi-user.target\n",
            name = service_register.service_name,
            user = service_register.user,
            work_dir = service_register.working_directory,
            bin_path = service_register.binary_path,
            args = args_str,
            env = env_str,
            restart = restart_str,
        );

        let temp_path = format!("/tmp/{}.service", service_register.service_name);
        let dest_path = format!(
            "/etc/systemd/system/{}.service",
            service_register.service_name
        );
        self.create_file(&temp_path, &service_content)?;
        self.ssh
            .run_cmd(&format!("sudo mv '{}' '{}'", temp_path, dest_path))?;
        self.ssh
            .run_cmd(&format!("sudo chown root:root '{}'", dest_path))?;
        self.ssh
            .run_cmd(&format!("sudo chmod 644 '{}'", dest_path))?;
        self.ssh.run_cmd("sudo systemctl daemon-reload")?;

        if service_register.auto_start {
            self.ssh.run_cmd(&format!(
                "sudo systemctl enable '{}'",
                service_register.service_name
            ))?;
            self.ssh.run_cmd(&format!(
                "sudo systemctl start '{}'",
                service_register.service_name
            ))?;
        }

        Ok(())
    }

    fn restart_service(&self, service_name: &str) -> anyhow::Result<()> {
        self.ssh
            .run_cmd(&format!("sudo systemctl restart '{}'", service_name))?;
        Ok(())
    }

    fn stop_service(&self, service_name: &str) -> anyhow::Result<()> {
        self.ssh
            .run_cmd(&format!("sudo systemctl stop '{}'", service_name))?;
        Ok(())
    }

    fn start_service(&self, service_name: &str) -> anyhow::Result<()> {
        self.ssh
            .run_cmd(&format!("sudo systemctl start '{}'", service_name))?;
        Ok(())
    }

    fn status_service(&self, service_name: &str) -> anyhow::Result<()> {
        self.ssh
            .run_cmd(&format!("sudo systemctl status '{}'", service_name))?;
        Ok(())
    }

    fn delete_service(&self, service_name: &str) -> anyhow::Result<()> {
        let service_path = format!("/etc/systemd/system/{}.service", service_name);
        let _ = self
            .ssh
            .run_cmd(&format!("sudo systemctl stop '{}'", service_name));
        let _ = self
            .ssh
            .run_cmd(&format!("sudo systemctl disable '{}'", service_name));
        self.ssh
            .run_cmd(&format!("sudo rm -f '{}'", service_path))?;
        self.ssh.run_cmd("sudo systemctl daemon-reload")?;
        Ok(())
    }

    fn install_dependencies(&self, dependencies: Vec<String>) -> anyhow::Result<()> {
        if dependencies.is_empty() {
            return Ok(());
        }
        let packages = dependencies.join(" ");
        self.ssh.run_cmd(&format!(
            "sudo apt-get update && sudo apt-get install -y {}",
            packages
        ))?;
        Ok(())
    }

    fn create_user(
        &self,
        user_register: super::server_interactor_trait::UserRegister,
    ) -> anyhow::Result<()> {
        // Check if user exists. If not, create it.
        if self
            .ssh
            .run_cmd(&format!("id -u {}", user_register.username))
            .is_err()
        {
            self.ssh.run_cmd(&format!(
                "sudo useradd -m -s /bin/bash {}",
                user_register.username
            ))?;
        }

        // Add to groups
        if !user_register.groups.is_empty() {
            self.add_user_to_groups(&user_register.username, user_register.groups)?;
        }

        // Setup SSH authorized keys if any
        if !user_register.ssh_authorized_keys.is_empty() {
            let keys_content = user_register.ssh_authorized_keys.join("\n");
            let temp_keys_path = format!("/tmp/crane-keys-{}", user_register.username);
            self.create_file(&temp_keys_path, &keys_content)?;

            let ssh_dir = format!("/home/{}/.ssh", user_register.username);
            let auth_keys_path = format!("{}/authorized_keys", ssh_dir);

            self.ssh.run_cmd(&format!("sudo mkdir -p '{}'", ssh_dir))?;
            self.ssh.run_cmd(&format!(
                "sudo mv '{}' '{}'",
                temp_keys_path, auth_keys_path
            ))?;
            self.ssh.run_cmd(&format!(
                "sudo chown -R '{}:{}' '{}'",
                user_register.username, user_register.username, ssh_dir
            ))?;
            self.ssh.run_cmd(&format!("sudo chmod 700 '{}'", ssh_dir))?;
            self.ssh
                .run_cmd(&format!("sudo chmod 600 '{}'", auth_keys_path))?;
        }

        Ok(())
    }

    fn delete_user(&self, username: &str) -> anyhow::Result<()> {
        self.ssh.run_cmd(&format!("sudo userdel -r {}", username))?;
        Ok(())
    }

    fn add_user_to_groups(&self, username: &str, groups: Vec<String>) -> anyhow::Result<()> {
        if groups.is_empty() {
            return Ok(());
        }
        for g in &groups {
            let _ = self.ssh.run_cmd(&format!("sudo groupadd '{}'", g));
        }
        let groups_csv = groups.join(",");
        self.ssh.run_cmd(&format!(
            "sudo usermod -a -G '{}' '{}'",
            groups_csv, username
        ))?;
        Ok(())
    }

    fn remove_user_from_groups(&self, username: &str, groups: Vec<String>) -> anyhow::Result<()> {
        if groups.is_empty() {
            return Ok(());
        }
        for g in &groups {
            let _ = self
                .ssh
                .run_cmd(&format!("sudo gpasswd -d '{}' '{}'", username, g));
        }
        Ok(())
    }
}
