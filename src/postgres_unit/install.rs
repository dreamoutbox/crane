use crate::server_interactor::server_interactor_trait::ServerInteractor;

pub fn install_postgres(interactor: &dyn ServerInteractor, version: &str) -> anyhow::Result<()> {
    let pg_ctl = format!("/usr/lib/postgresql/{}/bin/pg_ctl", version);
    let pg_installed = interactor
        .cmd(&format!("test -f {}", pg_ctl))
        .map(|out| out.exit_code == 0)
        .unwrap_or(false);

    if !pg_installed {
        println!("\tEnsuring GnuPG and Curl are installed...");
        interactor.install_dependencies(vec!["curl".to_string(), "gnupg".to_string()])?;

        println!(
            "Adding official PostgreSQL repository for version {}...",
            version
        );
        interactor.cmd("sudo rm -f /etc/apt/trusted.gpg.d/postgresql.gpg")?;
        interactor.cmd("sudo sh -c 'echo \"deb http://apt.postgresql.org/pub/repos/apt $(lsb_release -cs)-pgdg main\" > /etc/apt/sources.list.d/pgdg.list'")?;
        interactor.cmd("curl -fsSL https://www.postgresql.org/media/keys/ACCC4CF8.asc | sudo gpg --dearmor -o /etc/apt/trusted.gpg.d/postgresql.gpg")?;

        println!("\tUpdating package lists...");
        interactor.cmd("sudo apt-get update")?;

        println!(
            "\tInstalling postgresql-{} + client + python3-boto3...",
            version
        );
        interactor.install_dependencies(vec![
            format!("postgresql-{}", version),
            format!("postgresql-client-{}", version),
            "python3-boto3".to_string(),
        ])?;

        println!("\tEnabling PostgreSQL service for boot...");
        interactor.cmd("sudo systemctl enable postgresql")?;

        println!("\tStarting PostgreSQL cluster...");
        let start_cmd = format!(
            "sudo -u postgres {} -D /var/lib/postgresql/{}/main -o \"-c config_file=/etc/postgresql/{}/main/postgresql.conf -c restore_command=false\" start > /dev/null 2>&1 < /dev/null",
            pg_ctl, version, version
        );
        let _ = interactor.cmd(&start_cmd);
    }

    println!("\tPostgreSQL {} is already installed.", version);
    let main_dir = format!("/var/lib/postgresql/{}/main", version);
    let dir_exists = interactor
        .cmd(&format!("test -d {}", main_dir))
        .map(|out| out.exit_code == 0)
        .unwrap_or(false);

    if !dir_exists {
        println!(
            "\tPostgreSQL {} data directory is missing, initializing it...",
            version
        );
        let initdb_cmd = format!(
            "sudo -u postgres /usr/lib/postgresql/{}/bin/initdb -D {}",
            version, main_dir
        );
        interactor.cmd(&initdb_cmd)?;
    }
    let status_cmd = format!(
        "sudo -u postgres {} -D /var/lib/postgresql/{}/main status",
        pg_ctl, version
    );
    let is_running = interactor
        .cmd(&status_cmd)
        .map(|out| out.exit_code == 0)
        .unwrap_or(false);

    if !is_running {
        println!("\tPostgreSQL {} is stopped, starting it...", version);
        let start_cmd = format!(
            "sudo -u postgres {} -D /var/lib/postgresql/{}/main -o \"-c config_file=/etc/postgresql/{}/main/postgresql.conf -c restore_command=false\" start > /dev/null 2>&1 < /dev/null",
            pg_ctl, version, version
        );
        let _ = interactor.cmd(&start_cmd);
    }

    return Ok(());
}
