use crate::postgres_unit::PYTHON_PARSE_PG_LOG_SCRIPT;
use crate::postgres_unit::helper::{connect_to_node, postgres_get_primary};
use crate::server_interactor::server_interactor_trait::ServerInteractor;

pub fn run_postgres_logs_cmd(
    config: &crate::config::Config,
    since: Option<&str>,
    until: Option<&str>,
    user: Option<&str>,
    db: Option<&str>,
    sql: Option<&str>,
) -> anyhow::Result<()> {
    let primary_node = postgres_get_primary(config)?
        .ok_or_else(|| anyhow::anyhow!("No active PostgreSQL primary/leader found in cluster"))?;

    let interactor = connect_to_node(&primary_node, &config)?;

    let pg_logs = run_postgres_logs_wrapper(&*interactor, since, until, user, db, sql)?;

    if !pg_logs.trim().is_empty() {
        println!("{}", pg_logs);
    }

    Ok(())
}

pub fn run_postgres_logs_wrapper(
    interactor: &dyn ServerInteractor,
    since: Option<&str>,
    until: Option<&str>,
    user: Option<&str>,
    db: Option<&str>,
    sql: Option<&str>,
) -> anyhow::Result<String> {
    // Check if 'postgres' user exists on the remote node
    let user_check = interactor.cmd("id postgres")?;
    if user_check.exit_code != 0 {
        anyhow::bail!(
            "PostgreSQL is not installed or the 'postgres' user does not exist on the target node. 
Please run 'crane deploy' first to set up the database."
        );
    }

    // 1. Query the current CSV log path dynamically
    let log_path_cmd = "sudo -u postgres psql -tAc \"SELECT current_setting('data_directory') || '/' || pg_current_logfile('csvlog')\"";
    let log_path_output = interactor.cmd(log_path_cmd)?;
    if log_path_output.exit_code != 0 {
        anyhow::bail!(
            "Failed to query PostgreSQL log path: {}",
            log_path_output.stderr
        );
    }

    let log_file_path = log_path_output.stdout.trim();
    if log_file_path.is_empty() {
        anyhow::bail!(
            "PostgreSQL returned empty log path. Ensure logging_collector = on and log_destination = csvlog."
        );
    }

    // 2. Write the python parser script to the target node

    let script_path = "/tmp/crane_parse_pg_logs.py";
    interactor.create_file(script_path, PYTHON_PARSE_PG_LOG_SCRIPT)?;

    // 3. Construct and execute the Python command
    let mut py_cmd = format!(
        "sudo python3 {} '{}'",
        script_path,
        log_file_path.replace('\'', "'\\''")
    );
    if let Some(val) = since {
        py_cmd.push_str(&format!(" --since '{}'", val.replace('\'', "'\\''")));
    }
    if let Some(val) = until {
        py_cmd.push_str(&format!(" --until '{}'", val.replace('\'', "'\\''")));
    }
    if let Some(val) = user {
        py_cmd.push_str(&format!(" --user '{}'", val.replace('\'', "'\\''")));
    }
    if let Some(val) = db {
        py_cmd.push_str(&format!(" --db '{}'", val.replace('\'', "'\\''")));
    }
    if let Some(val) = sql {
        py_cmd.push_str(&format!(" --sql '{}'", val.replace('\'', "'\\''")));
    }

    let run_output = interactor.cmd(&py_cmd);

    // 4. Always clean up the temporary script
    let _ = interactor.cmd(&format!("rm -f {}", script_path));

    let output = run_output?;
    if output.exit_code != 0 {
        anyhow::bail!(
            "Failed to execute log parser (exit code {}): {}",
            output.exit_code,
            output.stderr
        );
    }

    Ok(output.stdout)
}
