use crate::postgres_unit::helper::connect_to_node;
use crate::postgres_unit::helper::find_node_config_with_fallback;
use crate::postgres_unit::helper::postgres_get_leader;
use crate::server_interactor::server_interactor_trait::ServerInteractor;

pub fn run_promote_cmd(
    config: &crate::config::Config,
    target_node_str: &str,
) -> anyhow::Result<()> {
    let target_conf = find_node_config_with_fallback(target_node_str, &config)
        .ok_or_else(|| anyhow::anyhow!("Node '{}' not found in configuration", target_node_str))?;

    if !target_conf.roles.contains(&"postgres".to_string()) {
        anyhow::bail!(
            "Node '{}' does not have the 'postgres' role",
            target_node_str
        );
    }

    let current_leader = postgres_get_leader(&config)?;

    if let Some(ref leader) = current_leader {
        if leader.internal_ip == target_conf.internal_ip {
            println!(
                "Node '{}' is already the active PostgreSQL leader.",
                target_node_str
            );
            return Ok(());
        }

        // Trigger switchover using patronictl
        println!(
            "\nSwitching over PostgreSQL leader from {} to {} using Patroni...",
            leader.name, target_conf.name
        );
        let target_interactor = connect_to_node(&target_conf, &config)?;
        let switch_cmd = format!(
            "sudo patronictl -c /etc/patroni/config.yml switchover --leader {} --candidate {} --scheduled now --force",
            leader.name, target_conf.name
        );
        let switch_output = target_interactor.cmd(&switch_cmd)?;

        if switch_output.exit_code != 0 {
            println!(
                "\nFailed to switchover PostgreSQL leader from {} to {}",
                leader.name, target_conf.name
            );

            println!("\nSTDOUT: {}", switch_output.stdout);
            println!("\nSTDERR: {}\n", switch_output.stderr);

            return Err(anyhow::anyhow!(
                "Failed to switchover PostgreSQL leader from {} to {}",
                leader.name,
                target_conf.name
            ));
        }
    } else {
        // Trigger failover using patronictl since there is no leader
        println!(
            "\nPromoting node {} to PostgreSQL leader using Patroni...",
            target_conf.name
        );

        let target_interactor = connect_to_node(&target_conf, &config)?;
        let failover_cmd = format!(
            "sudo patronictl -c /etc/patroni/config.yml failover --candidate {} --force",
            target_conf.name
        );
        let failover_output = target_interactor.cmd(&failover_cmd)?;

        if failover_output.exit_code != 0 {
            println!("\nFailed to failover PostgreSQL leader");

            println!("\nSTDOUT: {}", failover_output.stdout);
            println!("\nSTDERR\n: {}", failover_output.stderr);

            return Err(anyhow::anyhow!("Failed to failover PostgreSQL leader"));
        }
    }

    println!(
        "\nPROMOTION TO LEADER COMPLETE FOR NODE '{}'",
        target_conf.name
    );

    Ok(())
}

pub fn run_demote_cmd(config: &crate::config::Config, target_node_str: &str) -> anyhow::Result<()> {
    let target_conf = find_node_config_with_fallback(target_node_str, &config)
        .ok_or_else(|| anyhow::anyhow!("Node '{}' not found in configuration", target_node_str))?;

    if !target_conf.roles.contains(&"postgres".to_string()) {
        anyhow::bail!(
            "Node '{}' does not have the 'postgres' role",
            target_node_str
        );
    }

    let current_leader = postgres_get_leader(&config)?;

    let leader = current_leader.ok_or_else(|| {
        anyhow::anyhow!("Cannot demote: No active PostgreSQL leader discovered in the cluster to replicate from.")
    })?;

    if leader.internal_ip == target_conf.internal_ip {
        anyhow::bail!(
            "Node '{}' is currently the active leader. Demoting the leader directly is not permitted; please promote another node to leader instead.",
            target_node_str
        );
    }

    // Under Patroni, standbys follow the leader automatically. We can reinit the node if we want to force it to follow the leader cleanly.
    println!(
        "\nReinitializing Patroni standby node {} to ensure it follows leader {}...",
        target_conf.name, leader.name
    );
    let target_interactor = connect_to_node(&target_conf, &config)?;
    let reinit_cmd = format!(
        "sudo patronictl -c /etc/patroni/config.yml reinit postgres-cluster {} --force",
        target_conf.name
    );
    target_interactor.cmd(&reinit_cmd)?;

    println!("\nDEMOTION COMPLETE FOR NODE '{}'", target_conf.name);
    Ok(())
}

pub fn run_postgres_logs_cmd(
    config: &crate::config::Config,
    target_node_str: &str,
    since: Option<&str>,
    until: Option<&str>,
    user: Option<&str>,
    db: Option<&str>,
    sql: Option<&str>,
) -> anyhow::Result<()> {
    let target_conf = find_node_config_with_fallback(target_node_str, &config)
        .ok_or_else(|| anyhow::anyhow!("Node '{}' not found in configuration", target_node_str))?;

    if !target_conf.roles.contains(&"postgres".to_string()) {
        anyhow::bail!(
            "Node '{}' does not have the 'postgres' role",
            target_node_str
        );
    }

    let interactor = connect_to_node(&target_conf, &config)?;

    run_postgres_logs_cmd_internal(&*interactor, target_node_str, since, until, user, db, sql)
}

pub fn run_postgres_logs_cmd_internal(
    interactor: &dyn ServerInteractor,
    target_node_str: &str,
    since: Option<&str>,
    until: Option<&str>,
    user: Option<&str>,
    db: Option<&str>,
    sql: Option<&str>,
) -> anyhow::Result<()> {
    // Check if 'postgres' user exists on the remote node
    let user_check = interactor.cmd("id postgres")?;
    if user_check.exit_code != 0 {
        anyhow::bail!(
            "PostgreSQL is not installed or the 'postgres' user does not exist on node '{}'. Please run 'crane deploy' first to set up the database.",
            target_node_str
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
    let python_script = r#"import csv
import sys
import re
import argparse
from datetime import datetime

parser = argparse.ArgumentParser()
parser.add_argument('logfile')
parser.add_argument('--since', help='Filter since date (YYYY-MM-DD HH:MM:SS)')
parser.add_argument('--until', help='Filter until date (YYYY-MM-DD HH:MM:SS)')
parser.add_argument('--user', help='Filter by user name')
parser.add_argument('--db', help='Filter by database name')
parser.add_argument('--sql', help='Filter by SQL statement substring')
args = parser.parse_args()

dml_pattern = re.compile(r'\b(INSERT|UPDATE|DELETE|TRUNCATE)\b', re.IGNORECASE)

since_dt = None
if args.since:
    try:
        since_dt = datetime.fromisoformat(args.since.replace(' ', 'T'))
    except Exception:
        pass

until_dt = None
if args.until:
    try:
        until_dt = datetime.fromisoformat(args.until.replace(' ', 'T'))
    except Exception:
        pass

with open(args.logfile, 'r', encoding='utf-8', errors='replace') as f:
    reader = csv.reader(f)
    for row in reader:
        if len(row) < 14:
            continue
        
        log_time_str = row[0]
        user_name = row[1]
        database_name = row[2]
        client = row[4]
        severity = row[11]
        message = row[13]
        
        if severity == 'LOG' and message.startswith('statement: '):
            sql = message[len('statement: '):].strip()
            if not dml_pattern.search(sql):
                continue
            
            if args.user and args.user.lower() != user_name.lower():
                continue
            if args.db and args.db.lower() != database_name.lower():
                continue
            if args.sql and args.sql.lower() not in sql.lower():
                continue
            
            if since_dt or until_dt:
                try:
                    dt_part = log_time_str.split('.')[0]
                    row_dt = datetime.strptime(dt_part, '%Y-%m-%d %H:%M:%S')
                    if since_dt and row_dt < since_dt:
                        continue
                    if until_dt and row_dt > until_dt:
                        continue
                except Exception:
                    pass
            
            print(f"{log_time_str} | user={user_name} db={database_name} client={client} | SQL: {sql}")
"#;

    let script_path = "/tmp/crane_parse_pg_logs.py";
    interactor.create_file(script_path, python_script)?;

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

    if !output.stdout.trim().is_empty() {
        println!("{}", output.stdout);
    }

    Ok(())
}
