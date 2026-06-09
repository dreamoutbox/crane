use crate::{
    config::{Config, NodeConfig},
    haproxy_unit::haproxy::build_haproxy_config,
    patroni::build_patroni_config,
    server_interactor::server_interactor_trait::ServerInteractor,
    ssh::{CmdOutput, SSHSession},
};
use std::sync::Once;

static APT_UPDATE: Once = Once::new();

pub struct DebianInteractor {
    ssh: SSHSession,
    sudo_pass: Option<String>,
}

impl DebianInteractor {
    pub fn new(ssh: SSHSession, sudo_pass: Option<String>) -> Self {
        Self { ssh, sudo_pass }
    }

    fn wrap_sudo(&self, cmd: &str) -> String {
        if let Some(ref pass) = self.sudo_pass {
            if cmd.contains("sudo") {
                let mut escaped = String::new();
                for c in cmd.chars() {
                    match c {
                        '"' | '$' | '\\' | '`' => {
                            escaped.push('\\');
                            escaped.push(c);
                        }
                        _ => escaped.push(c),
                    }
                }
                format!("echo '{}' | sudo -S sh -c \"{}\"", pass, escaped)
            } else {
                cmd.to_string()
            }
        } else {
            cmd.to_string()
        }
    }

    fn run_stdout(&self, cmd: &str) -> anyhow::Result<String> {
        let cmd_to_run = self.wrap_sudo(cmd);
        let out = self.ssh.run_cmd(&cmd_to_run)?;

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
                "error DebianInteractor#runw executing command:\n{}\n(exit code: {})\n",
                cmd, out.exit_code
            );
            println!("STDOUT:\n{}\n", out.stdout);
            println!("STDERR:\n{}\n", out.stderr);

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
        self.run_stdout("whoami")
    }

    fn cmd(&self, cmd: &str) -> anyhow::Result<CmdOutput> {
        let cmd_to_run = self.wrap_sudo(cmd);
        let out = self.ssh.run_cmd(&cmd_to_run)?;

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
                let debug_cmd = std::env::var("DEBUG_CMD").unwrap_or_default();

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

    fn spawn_cmd(&self, cmd: &str) -> anyhow::Result<std::process::Child> {
        let cmd_to_run = self.wrap_sudo(cmd);
        self.ssh.spawn_cmd(&cmd_to_run)
    }

    fn get_os_info(&self) -> anyhow::Result<String> {
        self.run_stdout("uname -a")
    }

    fn server_paths(&self) -> crate::server_interactor::server_path::ServerPaths {
        crate::server_interactor::server_path::ServerPaths {
            // APP
            app_dir: "/opt/crane".to_string(),
            app_config_dir: "/etc/crane".to_string(),
            // PG
            pg_dir: "/usr/lib/postgresql".to_string(),
            pg_data_dir: "/var/lib/postgresql".to_string(),
            pg_bin_dir: "/usr/lib/postgresql".to_string(),
            pg_pass_path: "/var/lib/postgresql/.pgpass".to_string(),
            pg_backup_dir: "/var/lib/postgresql/backups".to_string(),
            pg_wal_archive: "/var/lib/postgresql/wal_archive".to_string(),
            // PATRONI
            patroni_config_path: "/etc/patroni/config.yml".to_string(),
            // HAPROXY
            haproxy_var_lib_dir: "/var/lib/haproxy".to_string(),
            haproxy_config_path: "/etc/haproxy/haproxy.cfg".to_string(),
            haproxy_log_dir: "/var/log/haproxy".to_string(),
        }
    }

    fn create_file(&self, path: &str, content: &str) -> anyhow::Result<()> {
        let b64 = crate::helper::base64::base64_encode(content);
        let cmd = format!(
            "echo '{}' | base64 -d | sudo tee '{}' > /dev/null",
            b64, path
        );
        self.run_stdout(&cmd)?;
        Ok(())
    }

    fn read_file(&self, path: &str) -> anyhow::Result<String> {
        self.run_stdout(&format!("sudo cat '{}'", path))
    }

    fn upload(&self, local_path: &str, remote_path: &str) -> anyhow::Result<()> {
        self.ssh.upload(local_path, remote_path)
    }

    fn download(&self, local_path: &str, remote_path: &str) -> anyhow::Result<()> {
        self.ssh.download(remote_path, local_path)
    }

    fn chmod(&self, path: &str, permission: &str) -> anyhow::Result<()> {
        self.run_stdout(&format!("sudo chmod -R '{}' '{}'", permission, path))?;
        Ok(())
    }

    fn chown(&self, path: &str, user: &str, group: &str) -> anyhow::Result<()> {
        self.run_stdout(&format!("sudo chown -R '{}:{}' '{}'", user, group, path))?;
        Ok(())
    }

    fn mkdir(&self, path: &str) -> anyhow::Result<()> {
        self.run_stdout(&format!("sudo mkdir -p '{}'", path))?;
        // self.run_checked(&format!("sudo chown '{}:{}' '{}'", user, group, path))?;
        Ok(())
    }

    fn ls(&self, path: &str) -> anyhow::Result<Vec<String>> {
        let output = self.run_stdout(&format!("ls -1 '{}'", path))?;
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
        self.run_stdout(&format!("sudo chown root:root '{}'", dest_path))?;
        self.run_stdout(&format!("sudo chmod 644 '{}'", dest_path))?;
        self.run_stdout("sudo systemctl daemon-reload")?;

        if service_register.auto_start {
            self.enable_service(&service_register.service_name)?;
            self.start_service(&service_register.service_name)?;
        }

        Ok(())
    }

    fn restart_service(&self, service_name: &str) -> anyhow::Result<()> {
        self.run_stdout(&format!("sudo systemctl restart {}", service_name))?;
        Ok(())
    }

    fn stop_service(&self, service_name: &str) -> anyhow::Result<()> {
        self.run_stdout(&format!("sudo systemctl stop {}", service_name))?;
        Ok(())
    }

    fn start_service(&self, service_name: &str) -> anyhow::Result<()> {
        self.run_stdout(&format!("sudo systemctl start {}", service_name))?;
        Ok(())
    }

    fn status_service(&self, service_name: &str) -> anyhow::Result<()> {
        self.run_stdout(&format!("sudo systemctl status {}", service_name))?;
        Ok(())
    }

    fn delete_service(&self, service_name: &str) -> anyhow::Result<()> {
        let service_path = format!("/etc/systemd/system/{}.service", service_name);
        self.stop_service(service_name)?;
        self.disable_service(service_name)?;
        self.run_stdout(&format!("sudo rm -f '{}'", service_path))?;
        self.service_daemon_reload()?;
        Ok(())
    }

    fn install_dependencies(&self, dependencies: Vec<String>) -> anyhow::Result<()> {
        if dependencies.is_empty() {
            return Ok(());
        }
        let packages = dependencies.join(" ");

        let mut update_res = Ok(());
        APT_UPDATE.call_once(|| {
            update_res = self.run_stdout("sudo apt-get update").map(|_| ());
        });
        update_res?;

        self.run_stdout(&format!("sudo apt-get install -y {}", packages))?;

        Ok(())
    }

    fn create_user(
        &self,
        user_register: super::server_interactor_trait::UserRegister,
    ) -> anyhow::Result<()> {
        if !self.user_exists(&user_register.username)? {
            let create_result = self.run_stdout(&format!(
                "sudo useradd -m -s /bin/bash {}",
                user_register.username
            ));

            match create_result {
                Ok(_) => {
                    println!("\tUser {} created successfully", user_register.username)
                }

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
        self.run_stdout(&format!(
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

            self.run_stdout(&format!("sudo mkdir -p '{}'", ssh_dir))?;

            self.create_file(&auth_keys_path, &keys_content)?;

            self.run_stdout(&format!(
                "sudo chown -R '{}:{}' '{}'",
                user_register.username, user_register.username, ssh_dir
            ))?;

            self.run_stdout(&format!("sudo chmod 700 '{}'", ssh_dir))?;
            self.run_stdout(&format!("sudo chmod 600 '{}'", auth_keys_path))?;
        }

        Ok(())
    }

    fn delete_user(&self, username: &str) -> anyhow::Result<()> {
        self.run_stdout(&format!("sudo userdel -r {}", username))?;
        Ok(())
    }

    fn add_user_to_groups(&self, username: &str, groups: Vec<String>) -> anyhow::Result<()> {
        if groups.is_empty() {
            return Ok(());
        }
        for g in &groups {
            let _ = self
                .ssh
                .run_cmd(&self.wrap_sudo(&format!("sudo groupadd '{}'", g)));
        }
        let groups_csv = groups.join(",");
        self.run_stdout(&format!(
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
                .run_cmd(&self.wrap_sudo(&format!("sudo gpasswd -d '{}' '{}'", username, g)));
        }
        Ok(())
    }

    fn list_users(&self) -> anyhow::Result<Vec<String>> {
        let output = self.run_stdout("cut -d: -f1 /etc/passwd")?;
        let users: Vec<String> = output
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Ok(users)
    }

    fn wait_for_service_start(&self, service_name: &str, timeout: u64) -> anyhow::Result<bool> {
        let start_time = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(timeout);
        let mut is_active = false;

        while start_time.elapsed() < timeout {
            let active_out = self.cmd(&format!("sudo systemctl is-active {}", service_name))?;

            if active_out.stdout.trim() == "active" {
                is_active = true;
                break;
            }

            std::thread::sleep(std::time::Duration::from_millis(500));
        }

        Ok(is_active)
    }

    fn service_daemon_reload(&self) -> anyhow::Result<()> {
        self.run_stdout("sudo systemctl daemon-reload")?;
        Ok(())
    }

    fn enable_service(&self, service_name: &str) -> anyhow::Result<()> {
        self.run_stdout(&format!("sudo systemctl enable {}", service_name))?;
        Ok(())
    }

    fn disable_service(&self, service_name: &str) -> anyhow::Result<()> {
        self.run_stdout(&format!("sudo systemctl disable {}", service_name))?;
        Ok(())
    }

    fn unzip(&self, zip_path: &str, dest: &str) -> anyhow::Result<()> {
        self.run_stdout(&format!("sudo unzip -o '{}' -d '{}'", zip_path, dest))?;

        Ok(())
    }

    fn firewall_enable(&self, enable: bool) -> anyhow::Result<()> {
        let is_installed = match self.cmd("which ufw") {
            Ok(out) => out.exit_code == 0,
            Err(_) => false,
        };

        if enable {
            if !is_installed {
                self.install_dependencies(vec!["ufw".to_string()])?;
            }
            self.run_stdout("sudo ufw default deny incoming")?;
            self.run_stdout("sudo ufw default allow outgoing")?;
            self.run_stdout("sudo ufw --force enable")?;
        } else if is_installed {
            self.run_stdout("sudo ufw disable")?;
        }

        Ok(())
    }

    fn firewall_reload(&self) -> anyhow::Result<()> {
        self.run_stdout("sudo ufw reload")?;
        Ok(())
    }

    fn firewall_reset(&self) -> anyhow::Result<()> {
        self.run_stdout("sudo ufw --force reset")?;
        Ok(())
    }

    fn firewall_allow_port(
        &self,
        port: u16,
        proto: &str,
        source: Option<&str>,
    ) -> anyhow::Result<()> {
        let cmd = match source {
            Some(s) => format!(
                "sudo ufw allow from {} to any port {} proto {}",
                s, port, proto
            ),
            None => format!("sudo ufw allow {}/{}", port, proto),
        };
        self.run_stdout(&cmd)?;
        Ok(())
    }

    fn firewall_deny_port(
        &self,
        port: u16,
        proto: &str,
        source: Option<&str>,
    ) -> anyhow::Result<()> {
        let cmd = match source {
            Some(s) => format!(
                "sudo ufw deny from {} to any port {} proto {}",
                s, port, proto
            ),
            None => format!("sudo ufw deny {}/{}", port, proto),
        };
        self.run_stdout(&cmd)?;
        Ok(())
    }

    fn firewall_allow_source(&self, source: &str) -> anyhow::Result<()> {
        self.run_stdout(&format!("sudo ufw allow from {}", source))?;
        Ok(())
    }

    fn mv(&self, src: &str, dest: &str) -> anyhow::Result<()> {
        self.run_stdout(&format!("sudo mv '{}' '{}'", src, dest))?;
        Ok(())
    }

    fn cp(&self, src: &str, dest: &str) -> anyhow::Result<()> {
        self.run_stdout(&format!("sudo cp -r '{}' '{}'", src, dest))?;
        Ok(())
    }

    fn exists(&self, path: &str) -> anyhow::Result<bool> {
        let out = self.cmd(&format!("sudo test -e '{}'", path))?;
        Ok(out.exit_code == 0)
    }

    fn rm(&self, path: &str) -> anyhow::Result<()> {
        let trimmed = path.trim();
        if trimmed.is_empty() || trimmed.chars().all(|c| c == '/') || trimmed == "/*" {
            anyhow::bail!(
                "Failsafe: Attempted to delete root directory or empty path: '{}'",
                path
            );
        }
        self.run_stdout(&format!("sudo rm -rf '{}'", path))?;
        Ok(())
    }

    fn tar_extract(&self, archive: &str, dest: &str) -> anyhow::Result<()> {
        self.run_stdout(&format!("sudo tar -xf '{}' -C '{}'", archive, dest))?;
        Ok(())
    }

    fn user_exists(&self, username: &str) -> anyhow::Result<bool> {
        let user_check = self
            .ssh
            .run_cmd(&self.wrap_sudo(&format!("id -u {}", username)))?;
        Ok(user_check.exit_code == 0)
    }

    fn which(&self, binary: &str) -> anyhow::Result<String> {
        let check = self.cmd(&format!("which {}", binary))?;
        if check.exit_code != 0 {
            anyhow::bail!("{} not found", binary);
        }
        Ok(check.stdout.trim().to_string())
    }

    fn check_http_status(&self, url: &str) -> anyhow::Result<u16> {
        let cmd = format!("curl -s -o /dev/null -w \"%{{http_code}}\" {}", url);
        let out = self.cmd(&cmd)?;
        let code = out.stdout.trim().parse::<u16>().unwrap_or(0);
        Ok(code)
    }

    fn update_etc_hosts(&self, entries: &[(String, String)]) -> anyhow::Result<()> {
        for (hostname, ip) in entries {
            // Filter out existing hostname entries,
            // append new IP mapping via a temp file,
            // then overwrite /etc/hosts.
            let cmd = format!(
                r#"sudo sh -c 'grep -v " {hostname}" /etc/hosts > /tmp/hosts.tmp && echo "{ip} {hostname}" >> /tmp/hosts.tmp && cp /tmp/hosts.tmp /etc/hosts && rm /tmp/hosts.tmp'"#,
                hostname = hostname,
                ip = ip
            );
            self.run_stdout(&cmd)?;
        }

        Ok(())
    }

    fn generate_self_signed_cert(
        &self,
        key_path: &str,
        crt_path: &str,
        cert_path: &str,
    ) -> anyhow::Result<()> {
        self.run_stdout(&format!(
            "sudo openssl req -x509 -nodes -days 365 -newkey rsa:2048 -keyout '{}' -out '{}' -subj '/CN=localhost'",
            key_path, crt_path
        ))?;
        self.run_stdout(&format!(
            "sudo sh -c 'cat \"{}\" \"{}\" > \"{}\"'",
            crt_path, key_path, cert_path
        ))?;
        self.chmod(cert_path, "600")?;
        Ok(())
    }

    fn wait_for_service_status(
        &self,
        service_name: &str,
        service_status: &str,
        timeout: u64,
    ) -> anyhow::Result<bool> {
        let start_time = std::time::Instant::now();
        let timeout_dur = std::time::Duration::from_secs(timeout);
        let mut met_status = false;

        while start_time.elapsed() < timeout_dur {
            let status = self.cmd(&format!("sudo systemctl is-active {}", service_name))?;
            if status.stdout.trim() == service_status {
                met_status = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1000));
        }
        Ok(met_status)
    }

    fn install_postgres(&self, version: &str) -> anyhow::Result<()> {
        let pg_ctl = self.pg_bin_path(version, "pg_ctl");
        let pg_installed = self.exists(&pg_ctl).unwrap_or(false);

        if !pg_installed {
            println!("\tEnsuring GnuPG and Curl are installed...");
            self.install_dependencies(vec!["curl".to_string(), "gnupg".to_string()])?;

            println!("\tAdding official PostgreSQL repository for version {version}");
            self.rm("/etc/apt/trusted.gpg.d/postgresql.gpg")?;
            self.cmd("sudo sh -c 'echo \"deb http://apt.postgresql.org/pub/repos/apt $(lsb_release -cs)-pgdg main\" > /etc/apt/sources.list.d/pgdg.list'")?;
            self.cmd("curl -fsSL https://www.postgresql.org/media/keys/ACCC4CF8.asc | sudo gpg --dearmor -o /etc/apt/trusted.gpg.d/postgresql.gpg")?;

            println!("\tUpdating package lists...");
            self.cmd("sudo apt-get update")?;

            println!(
                "\tInstalling postgresql-{version}, postgresql-client-{version}, python3-boto3"
            );
            self.install_dependencies(vec![
                format!("postgresql-{}", version),
                format!("postgresql-client-{}", version),
                "python3-boto3".to_string(),
            ])?;

            println!("\tEnabling PostgreSQL service for boot...");
            self.enable_service("postgresql")?;

            println!("\tStarting PostgreSQL cluster...");
            let start_cmd = format!(
                "sudo -u postgres {} -D /var/lib/postgresql/{}/main -o \"-c config_file=/etc/postgresql/{}/main/postgresql.conf -c restore_command=false\" start > /dev/null 2>&1 < /dev/null",
                pg_ctl, version, version
            );
            let _ = self.cmd(&start_cmd);
        } else {
            println!("\tPostgreSQL {} is already installed.", version);
        }

        Ok(())
    }

    fn kill_postgres_processes(&self) -> anyhow::Result<()> {
        let _ = self.cmd("sudo pkill -9 -u postgres postgres");
        Ok(())
    }

    fn pg_bin_path(&self, pg_version: &str, binary: &str) -> String {
        format!("/usr/lib/postgresql/{}/bin/{}", pg_version, binary)
    }

    fn psql(
        &self,
        command: Option<&str>,
        file: Option<&str>,
        dbname: Option<&str>,
        tuples_only: bool,
    ) -> anyhow::Result<CmdOutput> {
        let mut psql_cmd = "sudo -u postgres psql".to_string();
        if tuples_only {
            psql_cmd.push_str(" -t -A");
        }
        if let Some(db) = dbname {
            psql_cmd.push_str(&format!(" -d '{}'", db));
        }
        if let Some(c) = command {
            psql_cmd.push_str(&format!(
                " -c \"{}\"",
                c.replace('"', "\\\"").replace('$', "\\$")
            ));
        } else if let Some(f) = file {
            psql_cmd.push_str(&format!(" -f '{}'", f));
        }
        self.cmd(&psql_cmd)
    }

    fn setup_patroni(
        &self,
        node: &crate::config::NodeConfig,
        pg_version: &String,
        replica_pass: &String,
        pg_nodes: &Vec<crate::config::NodeConfig>,
    ) -> anyhow::Result<bool> {
        let patroni_installed = self.which("patroni").is_ok();

        if !patroni_installed {
            println!("\tInstalling Patroni...");
            self.install_dependencies(vec!["patroni".to_string()])?;
        } else {
            println!("\tPatroni is already installed.");
        }

        let paths = self.server_paths();
        let pg_data_dir = format!("{}/{}/main", paths.pg_data_dir, pg_version);
        let pg_bin_dir = format!("{}/{}/bin", paths.pg_bin_dir, pg_version);
        let patroni_yml = build_patroni_config(
            node,
            pg_version,
            replica_pass,
            pg_nodes,
            &pg_data_dir,
            &pg_bin_dir,
            &paths.pg_pass_path,
            &paths.pg_wal_archive,
        )?;
        std::fs::write(format!("patroni_{}.yaml", node.name), patroni_yml.clone())?;

        let patroni_path = self.server_paths().patroni_config_path;
        let existing_config = self.read_file(&patroni_path).unwrap_or_default();
        let config_changed = existing_config.trim() != patroni_yml.trim();

        self.mkdir("/etc/patroni")?;
        self.create_file(&patroni_path, &patroni_yml)?;
        self.chown(&patroni_path, "postgres", "postgres")?;
        self.chmod(&patroni_path, "600")?;
        println!("\tCreate patroni config at {}", patroni_path);

        // let patroni_config_yml_exists = self.exists(&patroni_path)?;
        // dbg!(&patroni_path, patroni_config_yml_exists);

        if !patroni_installed {
            self.service_daemon_reload()?;
            self.enable_service("patroni")?;
        }

        Ok(config_changed)
    }

    fn setup_etcd(&self, node: &NodeConfig, pg_nodes: &[NodeConfig]) -> anyhow::Result<()> {
        println!("\tSetup etcd cluster on node {}...", node.name);

        let etcd_installed = self.which("etcd").is_ok();
        if !etcd_installed {
            println!("\tInstalling etcd-server and etcd-client...");
            self.install_dependencies(vec!["etcd-server".to_string(), "etcd-client".to_string()])?;
        } else {
            println!("\tetcd is already installed.");
        }

        let etcd_configured = self.exists("/etc/default/etcd").unwrap_or(false);
        if !etcd_configured {
            // Stop etcd cleanly and remove data directory; wait to ensure it is fully stopped
            let _ = self.stop_service("etcd");
            let _ = self.wait_for_service_status("etcd", "inactive", 30);
            let _ = self.rm("/var/lib/etcd/");

            // Recreate with correct ownership so the etcd service user can write to it
            self.mkdir("/var/lib/etcd")?;
            self.chown("/var/lib/etcd", "etcd", "etcd")?;
            self.chmod("/var/lib/etcd", "700")?;
        }

        let initial_cluster = pg_nodes
            .iter()
            .map(|n| format!("{}=http://{}:2380", n.name, n.internal_ip))
            .collect::<Vec<_>>()
            .join(",");

        // Use "existing" if etcd member data already exists to avoid re-triggering
        // Patroni DCS re-bootstrap (and pg_basebackup) on every redeploy.
        let has_etcd_data = self
            .exists("/var/lib/etcd/default.etcd/member")
            .unwrap_or(false);
        let cluster_state = if has_etcd_data { "existing" } else { "new" };

        let etcd_default = format!(
            r#"
# Member settings
ETCD_NAME="{etcd_name}"
ETCD_DATA_DIR="/var/lib/etcd/default.etcd"
ETCD_LISTEN_PEER_URLS="http://0.0.0.0:2380"
ETCD_LISTEN_CLIENT_URLS="http://0.0.0.0:2379"

# Clustering settings
ETCD_INITIAL_ADVERTISE_PEER_URLS="http://{internal_ip}:2380"
ETCD_INITIAL_CLUSTER="{initial_cluster}"
ETCD_INITIAL_CLUSTER_STATE="{cluster_state}"
ETCD_INITIAL_CLUSTER_TOKEN="etcd-postgres-token"
ETCD_ADVERTISE_CLIENT_URLS="http://{internal_ip}:2379"
"#,
            etcd_name = node.name,
            internal_ip = node.internal_ip,
            initial_cluster = initial_cluster,
            cluster_state = cluster_state,
        );

        // println!("\nETCD CONFIG\n{}\n", &etcd_default);

        let etcd_default_path = "/etc/default/etcd";
        self.create_file(etcd_default_path, &etcd_default)?;
        self.chown(etcd_default_path, "root", "root")?;
        self.chmod(etcd_default_path, "644")?;

        if !etcd_installed {
            self.service_daemon_reload()?;
            self.enable_service("etcd")?;
        }

        Ok(())
    }

    /// Start etcd non-blocking. Call after all nodes are configured so the cluster forms together.
    fn start_etcd(&self, node: &NodeConfig) -> anyhow::Result<()> {
        // Check the node's internal IP (not localhost) so we only skip restart when etcd is
        // actually bound to the correct interface.
        // Checking localhost would pass even if etcd is running
        // with an old config that only binds 127.0.0.1.
        let check_cmd = format!(
            "env ETCDCTL_API=3 etcdctl --endpoints=http://{}:2379 endpoint health",
            node.internal_ip
        );
        let is_healthy = self
            .cmd(&check_cmd)
            .map(|o| o.exit_code == 0)
            .unwrap_or(false);

        if is_healthy {
            println!(
                "\tetcd already healthy on node {}, skipping restart",
                node.name
            );
            return Ok(());
        }

        println!("\tStarting etcd service on node {} ...", node.name);
        self.restart_service("etcd --no-block")?;

        Ok(())
    }

    fn setup_firewall(&self, config: &Config) -> anyhow::Result<()> {
        let mut allow_sources = String::new();
        for node in &config.nodes {
            allow_sources.push_str(&format!("ufw allow from {}\n", node.internal_ip));
        }

        let script_content = format!(
            r#"#!/bin/bash
set -e

# Ensure ufw is installed
if ! command -v ufw >/dev/null 2>&1; then
    export DEBIAN_FRONTEND=noninteractive
    apt-get update && apt-get install -y ufw
fi

# 1. Reset firewall rules to defaults
ufw --force reset

# 2. Allow SSH before enabling to avoid lockouts
ufw allow 22/tcp

# 3. Enable firewall
ufw default deny incoming
ufw default allow outgoing
ufw --force enable

# 4. Allow public HTTP/HTTPS traffic
ufw allow 80/tcp
ufw allow 443/tcp

# 5. Allow all incoming traffic from internal IPs of all nodes in the cluster
{}
# 6. Reload firewall to apply rules
ufw reload
"#,
            allow_sources
        );

        self.mkdir("/tmp/crane")?;
        self.create_file("/tmp/crane/setup_firewall.sh", &script_content)?;
        self.chmod("/tmp/crane/setup_firewall.sh", "+x")?;
        self.run_stdout("sudo bash /tmp/crane/setup_firewall.sh")?;

        Ok(())
    }

    fn setup_haproxy(&self, config: &Config) -> anyhow::Result<()> {
        let haproxy_installed = self.which("haproxy").is_ok();

        // Check if HAProxy binary is already installed
        if !haproxy_installed {
            println!("\tInstalling HAProxy on remote server...");
            self.install_dependencies(vec!["haproxy".to_string()])?;
        }

        let cert_path = debian_ensure_self_signed_cert(&*self)?;

        let haproxy_config_path = "/etc/haproxy/haproxy.cfg";
        let haproxy_config_data = build_haproxy_config(config, cert_path)?;

        // Write config to remote node
        self.create_file(haproxy_config_path, &haproxy_config_data)?;
        self.chown(haproxy_config_path, "root", "root")?;
        self.chmod(haproxy_config_path, "644")?;
        println!("\tUpdate HAProxy config (at: {})", haproxy_config_path);

        // Write to local file for debug
        std::fs::write("haproxy.cfg", &haproxy_config_data)?;

        Ok(())
    }

    fn reload_haproxy(&self) -> anyhow::Result<()> {
        self.service_daemon_reload()?;
        self.enable_service("haproxy")?;
        self.restart_service("haproxy")?;
        Ok(())
    }

    fn setup_systemd_template(
        &self,
        app_name: &str,
        deploy_user: &str,
        entrypoint: &str,
        env_path: &str,
    ) -> anyhow::Result<()> {
        let service_file_path = format!("/etc/systemd/system/{}@.service", app_name);
        let clean_entrypoint = entrypoint.trim_start_matches("./");
        let working_dir = format!("/app/{}", app_name);
        let exec_start = format!("/app/{}/{}", app_name, clean_entrypoint);

        println!(
            "\t[{app_name}] Register Service:
\t\tPath={service_file_path}
\t\tUser={deploy_user}
\t\tWorkingDirectory={working_dir}
\t\tExecStart={exec_start}
\t\tEnvFile={env_path}"
        );

        let systemd_data = format!(
            r#"[Unit]
Description=crane managed: %p instance on port %i
After=network.target

[Service]
Type=simple
User={deploy_user}
WorkingDirectory={working_dir}
ExecStart={exec_start}
EnvironmentFile={env_path}
Restart=on-failure
RestartSec=5
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true

[Install]
WantedBy=multi-user.target
"#,
            deploy_user = deploy_user,
            working_dir = working_dir,
            exec_start = exec_start,
            env_path = env_path,
        );

        self.create_file(&service_file_path, &systemd_data)?;

        self.chown(&service_file_path, "root", "root")?;
        self.chmod(&service_file_path, "644")?;

        let _ = self.service_daemon_reload()?;

        Ok(())
    }
}

fn debian_ensure_self_signed_cert(interactor: &dyn ServerInteractor) -> anyhow::Result<String> {
    let cert_dir = "/etc/ssl/private";

    let cert_path = "/etc/ssl/private/crane_self_signed.pem";
    let key_path = "/etc/ssl/private/crane_self_signed.key";
    let crt_path = "/etc/ssl/private/crane_self_signed.crt";

    // Ensure cert directory exists
    interactor.mkdir(cert_dir)?;

    // Check if the pem file exists
    if !interactor.exists(cert_path)? {
        println!("\tGenerating self-signed certificate for SSL/TLS termination...");
        interactor.generate_self_signed_cert(key_path, crt_path, cert_path)?;
    }

    Ok(cert_path.to_string())
}
