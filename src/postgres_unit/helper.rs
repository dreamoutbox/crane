use crate::config;
use crate::server_interactor::server_interactor_trait::ServerInteractor;

pub fn get_pg_version(config: &config::Config) -> String {
    config
        .db
        .as_ref()
        .and_then(|db| db.postgres.as_ref())
        .and_then(|pg| pg.get("version"))
        .and_then(|val| val.as_str())
        .unwrap_or("16")
        .to_string()
}

pub fn get_replica_pass(dot_env: &std::collections::HashMap<String, String>) -> String {
    dot_env
        .get("POSTGRES_PASSWORD")
        .cloned()
        .unwrap_or_else(|| "repl_password".to_string())
}

pub fn is_postgres_running(interactor: &dyn ServerInteractor, version: &str) -> bool {
    let pg_ctl = format!("/usr/lib/postgresql/{}/bin/pg_ctl", version);
    let status_cmd = format!(
        "sudo -u postgres {} -D /var/lib/postgresql/{}/main status",
        pg_ctl, version
    );
    interactor.cmd(&status_cmd).is_ok()
}

pub fn start_postgres(interactor: &dyn ServerInteractor, version: &str) -> anyhow::Result<()> {
    let pg_ctl = format!("/usr/lib/postgresql/{}/bin/pg_ctl", version);
    let postgres_start_cmd = format!(
        "sudo -u postgres {} -D /var/lib/postgresql/{}/main -o \"-c config_file=/etc/postgresql/{}/main/postgresql.conf\" start > /dev/null 2>&1 < /dev/null",
        pg_ctl, version, version
    );

    let out = interactor.cmd(&postgres_start_cmd)?;
    if out.exit_code != 0 {
        println!(
            "error executing postgres_start_cmd {} (exit code: {})",
            postgres_start_cmd, out.exit_code
        );
        println!("\nSTDERR: \n\n{}\n\n", out.stderr);

        anyhow::bail!(
            "Command '{}' failed with exit code {}: {}",
            postgres_start_cmd,
            out.exit_code,
            out.stderr
        );
    }

    Ok(())
}

pub fn ensure_postgres_running(interactor: &dyn ServerInteractor, version: &str) {
    //-> anyhow::Result<()>
    if is_postgres_running(interactor, version) {
        // return Ok(());
        return;
    }

    println!("\tPostgreSQL {} is stopped, starting it...", version);
    let _ = start_postgres(interactor, version);

    for _ in 0..20 {
        if is_postgres_running(interactor, version) {
            // return Ok(());
            return;
        }

        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    eprintln!(
        "Error: PostgreSQL {} is not running and could not be started",
        version
    );
    std::process::exit(1);

    // anyhow::bail!(
    //     "PostgreSQL {} failed to start or respond to status check",
    //     version
    // )
}
