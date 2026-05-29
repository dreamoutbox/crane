use std::path::Path;

use crate::config;
use crate::postgres_unit::entity::PostgresNode;
use crate::postgres_unit::helper::connect_to_node;
use crate::postgres_unit::helper::find_node_config_with_fallback;
use crate::postgres_unit::helper::get_backups_from_s3;
use crate::postgres_unit::helper::postgres_get_leader;
use crate::s3::get_s3_config;
use crate::s3::s3_client::RealS3Client;
use crate::server_interactor::server_interactor_trait::ServerInteractor;

pub fn run_promote_cmd(config_path: &Path, target_node_str: &str) -> anyhow::Result<()> {
    let config = config::load_config(config_path)?;

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
            "sudo patronictl -c /etc/patroni/config.yml switchover --master {} --candidate {} --scheduled now --force",
            leader.name, target_conf.name
        );
        target_interactor.cmd(&switch_cmd)?;
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
        target_interactor.cmd(&failover_cmd)?;
    }

    println!(
        "\nPROMOTION TO LEADER COMPLETE FOR NODE '{}'",
        target_conf.name
    );
    Ok(())
}

pub fn run_demote_cmd(config_path: &Path, target_node_str: &str) -> anyhow::Result<()> {
    let config = config::load_config(config_path)?;

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

pub fn run_status_cmd(config_path: &Path) -> anyhow::Result<()> {
    let config = config::load_config(config_path)?;

    let pg_nodes: Vec<_> = config
        .nodes
        .iter()
        .filter(|n| n.roles.contains(&"postgres".to_string()))
        .cloned()
        .collect();

    let pg_version_config = config
        .db
        .as_ref()
        .and_then(|db| db.postgres.as_ref())
        .and_then(|pg| pg.get("version"))
        .and_then(|val| val.as_str())
        .unwrap_or("16")
        .to_string();

    let mut statuses = Vec::new();
    let mut primary_host = "Unknown".to_string();

    for node in &pg_nodes {
        let address = format!("{}:{}", node.public_ip, node.port);
        let mut hostname = node.host.clone();
        let mut role = "Unknown".to_string();
        let mut version = pg_version_config.clone();
        let mut health = "Unhealthy".to_string();

        match connect_to_node(node, &config) {
            Ok(interactor) => {
                // 1. Get Hostname
                if let Ok(h) = interactor.cmd("hostname") {
                    let h_trimmed = h.stdout.trim();
                    if !h_trimmed.is_empty() {
                        hostname = h_trimmed.to_string();
                    }
                }

                // 2. Check Recovery & DB Version
                let recovery_cmd =
                    r#"sudo -u postgres psql -t -A -c "select pg_is_in_recovery();""#;
                let version_cmd = r#"sudo -u postgres psql -t -A -c "show server_version;""#;

                let is_recovery = interactor.cmd(recovery_cmd);
                let db_ver_str = interactor.cmd(version_cmd);

                if let Ok(rec) = is_recovery {
                    let rec_trimmed = rec.stdout.trim();
                    if rec_trimmed == "f" {
                        role = "Leader".to_string();
                        primary_host = hostname.clone();
                        health = "Healthy".to_string();
                    } else if rec_trimmed == "t" {
                        role = "Follower".to_string();
                        health = "Healthy".to_string();
                    }
                }

                if let Ok(v_str) = db_ver_str {
                    let v_trimmed = v_str.stdout.trim();
                    if let Some(major) = v_trimmed.split('.').next() {
                        let major_clean = major.trim();
                        if !major_clean.is_empty() {
                            version = major_clean.to_string();
                        }
                    }
                }
            }

            Err(_) => {
                // SSH connection failure defaults to Unhealthy
            }
        }

        statuses.push(PostgresNode {
            hostname,
            address,
            role,
            version,
            health,
        });
    }

    // Identify backups (all postgres nodes that are not the leader)
    let mut backups = Vec::new();
    for status in &statuses {
        if status.hostname != primary_host {
            backups.push(format!("{}:5000", status.hostname));
        }
    }

    // Print expected output format
    println!("\nHAProxy");
    println!("Primary: {}:5000", primary_host);
    println!("Backup: {}", backups.join(","));

    for status in &statuses {
        println!("\n{}", status.hostname);
        println!("Address: {}", status.address);
        println!("Role: {}", status.role);
        println!("DB version: {}", status.version);
        println!("Health: {}", status.health);
    }
    println!();

    Ok(())
}

pub fn run_list_backups_cmd(config_path: &Path) -> anyhow::Result<()> {
    let config = config::load_config(config_path)?;
    let config_dir = config_path.parent().unwrap_or(Path::new("."));
    let env_path = config_dir.join(".env");
    let dot_env = config::load_env_file(&env_path).unwrap_or_default();

    let s3_config = get_s3_config(&config, &dot_env)?;
    let s3_client = RealS3Client::new(&s3_config)?;

    let backups = get_backups_from_s3(&s3_client)?;

    if backups.is_empty() {
        println!("No backups found in cluster.");
        return Ok(());
    }

    for (idx, backup) in backups.iter().enumerate() {
        if idx > 0 {
            println!();
        }

        println!("ID: {}", backup.id);
        println!("Date: {}", backup.date);
        println!("Time: {}", backup.time);
        println!("Type: {}", backup.backup_type);
        println!("LOCAL: {}", backup.local_path);
        println!("S3: {}", backup.s3_path);
    }

    Ok(())
}

pub fn run_postgres_logs_cmd(
    config_path: &Path,
    target_node_str: &str,
    since: Option<&str>,
    until: Option<&str>,
    user: Option<&str>,
    db: Option<&str>,
    sql: Option<&str>,
) -> anyhow::Result<()> {
    let config = config::load_config(config_path)?;
    let target_conf = find_node_config_with_fallback(target_node_str, &config)
        .ok_or_else(|| anyhow::anyhow!("Node '{}' not found in configuration", target_node_str))?;

    if !target_conf.roles.contains(&"postgres".to_string()) {
        anyhow::bail!(
            "Node '{}' does not have the 'postgres' role",
            target_node_str
        );
    }

    let interactor = connect_to_node(&target_conf, &config)?;

    run_postgres_logs_cmd_internal(
        &*interactor,
        target_node_str,
        since,
        until,
        user,
        db,
        sql,
    )
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
