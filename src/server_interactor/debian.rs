use crate::{
    server_interactor::server_interactor_trait::ServerInteractor,
    ssh::{CmdOutput, SSHSession},
};
use std::sync::Once;

static APT_UPDATE: Once = Once::new();

pub struct DebianInteractor {
    ssh: SSHSession,
}

impl DebianInteractor {
    pub fn new(ssh: SSHSession) -> Self {
        Self { ssh }
    }

    fn run_checked(&self, cmd: &str) -> anyhow::Result<String> {
        let out = self.ssh.run_cmd(cmd)?;

        if out.exit_code != 0 {
            // if error like:
            // error executing command sudo systemctl stop 'myapp2@4000' (exit code: 5)
            // with stderr like:
            // Failed to stop myapp2@4000.service: Unit myapp2@4000.service not loaded.
            // then skip
            if cmd.contains("systemctl stop") && out.stderr.contains("not loaded") {
                return Ok(String::new());
            }

            println!(
                "error executing command {} (exit code: {})",
                cmd, out.exit_code
            );
            println!("\nDebianInteractor run_checked STDERR:\n{}\n", out.stderr);

            anyhow::bail!(
                "Command '{}' failed with exit code {}: {}",
                cmd,
                out.exit_code,
                out.stderr
            );
        }

        Ok(out.stdout)
    }
}

impl ServerInteractor for DebianInteractor {
    fn whoami(&self) -> anyhow::Result<String> {
        self.run_checked("whoami")
    }

    fn cmd(&self, cmd: &str) -> anyhow::Result<CmdOutput> {
        let out = self.ssh.run_cmd(cmd)?;

        if out.exit_code != 0 {
            // if error is error executing command sudo systemctl stop 'myapp2@4000' (exit code: 5)
            // with stderr like:
            // Failed to stop myapp2@4000.service: Unit myapp2@4000.service not loaded.
            // then skip
            if cmd.contains("systemctl stop")
                || cmd.contains("test -d")
                || out.stderr.contains("not loaded")
            {
            } else {
                let debug_cmd = std::env::var("DEBUG_CMD_ERROR").unwrap_or_default();

                if !debug_cmd.is_empty() {
                    println!("=========================");
                    println!(
                        "Error DebianInteractor#cmd executing command:\n{}\n(exit code: {})",
                        cmd, out.exit_code
                    );
                    println!("DebianInteractor#cmd STDOUT:\n{}", out.stdout);
                    println!("DebianInteractor#cmd STDERR:\n{}", out.stderr);
                    println!("=========================");
                }
            }
        }

        Ok(out)
    }

    fn get_os_info(&self) -> anyhow::Result<String> {
        self.run_checked("uname -a")
    }

    fn create_file(&self, path: &str, content: &str) -> anyhow::Result<()> {
        let b64 = crate::helper::base64::base64_encode(content);
        let cmd = format!(
            "echo '{}' | base64 -d | sudo tee '{}' > /dev/null",
            b64, path
        );
        self.run_checked(&cmd)?;
        Ok(())
    }

    fn read_file(&self, path: &str) -> anyhow::Result<String> {
        self.run_checked(&format!("cat '{}'", path))
    }

    fn upload(&self, local_path: &str, remote_path: &str) -> anyhow::Result<()> {
        self.ssh.upload(local_path, remote_path)
    }

    fn download(&self, local_path: &str, remote_path: &str) -> anyhow::Result<()> {
        self.ssh.download(remote_path, local_path)
    }

    fn chmod(&self, path: &str, permission: &str) -> anyhow::Result<()> {
        self.run_checked(&format!("chmod -R '{}' '{}'", permission, path))?;
        Ok(())
    }

    fn chown(&self, path: &str, user: &str, group: &str) -> anyhow::Result<()> {
        self.run_checked(&format!("chown -R '{}:{}' '{}'", user, group, path))?;
        Ok(())
    }

    fn mkdir(&self, path: &str, user: &str, group: &str) -> anyhow::Result<()> {
        self.run_checked(&format!("sudo mkdir -p '{}'", path))?;
        self.run_checked(&format!("sudo chown '{}:{}' '{}'", user, group, path))?;
        Ok(())
    }

    fn ls(&self, path: &str) -> anyhow::Result<Vec<String>> {
        let output = self.run_checked(&format!("ls -1 '{}'", path))?;
        let files: Vec<String> = output
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Ok(files)
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

        let dest_path = format!(
            "/etc/systemd/system/{}.service",
            service_register.service_name
        );
        self.create_file(&dest_path, &service_content)?;
        self.run_checked(&format!("sudo chown root:root '{}'", dest_path))?;
        self.run_checked(&format!("sudo chmod 644 '{}'", dest_path))?;
        self.run_checked("sudo systemctl daemon-reload")?;

        if service_register.auto_start {
            self.run_checked(&format!(
                "sudo systemctl enable '{}'",
                service_register.service_name
            ))?;
            self.run_checked(&format!(
                "sudo systemctl start '{}'",
                service_register.service_name
            ))?;
        }

        Ok(())
    }

    fn restart_service(&self, service_name: &str) -> anyhow::Result<()> {
        self.run_checked(&format!("sudo systemctl restart '{}'", service_name))?;
        Ok(())
    }

    fn stop_service(&self, service_name: &str) -> anyhow::Result<()> {
        self.run_checked(&format!("sudo systemctl stop '{}'", service_name))?;
        Ok(())
    }

    fn start_service(&self, service_name: &str) -> anyhow::Result<()> {
        self.run_checked(&format!("sudo systemctl start '{}'", service_name))?;
        Ok(())
    }

    fn status_service(&self, service_name: &str) -> anyhow::Result<()> {
        self.run_checked(&format!("sudo systemctl status '{}'", service_name))?;
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
        self.run_checked(&format!("sudo rm -f '{}'", service_path))?;
        self.run_checked("sudo systemctl daemon-reload")?;
        Ok(())
    }

    fn install_dependencies(&self, dependencies: Vec<String>) -> anyhow::Result<()> {
        if dependencies.is_empty() {
            return Ok(());
        }
        let packages = dependencies.join(" ");

        let mut update_res = Ok(());
        APT_UPDATE.call_once(|| {
            update_res = self.run_checked("sudo apt-get update").map(|_| ());
        });
        update_res?;

        self.run_checked(&format!("sudo apt-get install -y {}", packages))?;

        Ok(())
    }

    fn create_user(
        &self,
        user_register: super::server_interactor_trait::UserRegister,
    ) -> anyhow::Result<()> {
        // Check if user exists. If not, create it.
        let user_check = self
            .ssh
            .run_cmd(&format!("id -u {}", user_register.username))?;
        if user_check.exit_code != 0 {
            let create_result = self.run_checked(&format!(
                "sudo useradd -m -s /bin/bash {}",
                user_register.username
            ));

            match create_result {
                Ok(_) => println!("\tUser {} created successfully", user_register.username),

                Err(e) => {
                    if e.to_string().contains("already exists") {
                        println!("User already exists, no update");
                    } else {
                        anyhow::bail!("Failed to create user {}: {}", user_register.username, e);
                    }
                }
            }
        }

        // Ensure home directory ownership is correct (in case it existed beforehand)
        self.run_checked(&format!(
            "sudo chown '{username}:{username}' '/home/{username}'",
            username = user_register.username
        ))?;

        // Add to groups
        if !user_register.groups.is_empty() {
            self.add_user_to_groups(&user_register.username, user_register.groups)?;
        }

        // Setup SSH authorized keys if any
        if !user_register.ssh_authorized_keys.is_empty() {
            let keys_content = user_register.ssh_authorized_keys.join("\n");
            let ssh_dir = format!("/home/{}/.ssh", user_register.username);
            let auth_keys_path = format!("{}/authorized_keys", ssh_dir);

            self.run_checked(&format!("sudo mkdir -p '{}'", ssh_dir))?;

            self.create_file(&auth_keys_path, &keys_content)?;

            self.run_checked(&format!(
                "sudo chown -R '{}:{}' '{}'",
                user_register.username, user_register.username, ssh_dir
            ))?;

            self.run_checked(&format!("sudo chmod 700 '{}'", ssh_dir))?;
            self.run_checked(&format!("sudo chmod 600 '{}'", auth_keys_path))?;
        }

        Ok(())
    }

    fn delete_user(&self, username: &str) -> anyhow::Result<()> {
        self.run_checked(&format!("sudo userdel -r {}", username))?;
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
        self.run_checked(&format!(
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

    fn list_users(&self) -> anyhow::Result<Vec<String>> {
        let output = self.run_checked("cut -d: -f1 /etc/passwd")?;
        let users: Vec<String> = output
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Ok(users)
    }
}
