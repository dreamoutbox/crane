use crate::{config::{PostgresDbConfig, PostgresUserConfig}, server_interactor::server_interactor_trait::ServerInteractor};


pub async fn setup_postgres_primary(
    interactor: std::sync::Arc<dyn ServerInteractor + Send + Sync>,
    _version: &str,
    _replica_pass: &str,
    db_configs: &[PostgresDbConfig],
    user_configs: &[PostgresUserConfig],
    _config: &crate::config::Config,
) -> anyhow::Result<()> {
    println!("\n\tProvisioning PostgreSQL databases and users on Patroni leader...");

    let interactor_clone = interactor.clone();
    let db_configs = db_configs.to_vec();
    let user_configs = user_configs.to_vec();

    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let run_psql = |cmd: &str| -> anyhow::Result<crate::ssh::CmdOutput> {
            let out = interactor_clone.cmd(cmd)?;
            if out.exit_code != 0 {
                anyhow::bail!("psql command failed: {}\nStderr: {}", cmd, out.stderr);
            }
            Ok(out)
        };


        
        // Idempotently create databases sequentially
        for db in &db_configs {
            println!("\n\tSetting up database '{}'...", db.name);

            let check_db_sql = format!("SELECT 1 FROM pg_database WHERE datname = '{}'", db.name);
            let db_exists = run_psql(&format!(
                "sudo -u postgres psql -t -A -c \"{}\"",
                check_db_sql
            ))?;

            if db_exists.stdout.trim() != "1" {
                run_psql(&format!(
                    "sudo -u postgres psql -c \"CREATE DATABASE {};\"",
                    db.name
                ))?;
            }
        }

        // Idempotently create/remove users and grant/revoke privileges sequentially
        for user in &user_configs {
            let user_state = user.state.as_deref().unwrap_or("present");

            println!("\tuser {} state is {}", user.user, user_state);

            if user_state == "absent" {
                println!("\tRemoving user '{}'...", user.user);

                for db_ref in &user.databases {
                    let db_name = db_configs
                        .iter()
                        .find(|d| &d.name == db_ref)
                        .map(|d| d.name.as_str())
                        .unwrap_or(db_ref);

                    println!(
                        "\tRevoking privileges for user '{}' on database '{}'...",
                        user.user, db_name
                    );

                    let _ = interactor_clone.cmd(&format!(
                        "sudo -u postgres psql -d {} -c \"REVOKE ALL ON SCHEMA public FROM {};\"",
                        db_name, user.user
                    ));

                    let _ = interactor_clone.cmd(&format!(
                        "sudo -u postgres psql -c \"REVOKE ALL PRIVILEGES ON DATABASE {} FROM {};\"",
                        db_name, user.user
                    ));
                }

                run_psql(&format!(
                    "sudo -u postgres psql -c \"DROP ROLE IF EXISTS {};\"",
                    user.user
                ))?;
            } else if user_state == "present" {
                println!("\tSetting up user '{}'...", user.user);

                // Write SQL to temp file to avoid shell quoting issues with $$ and newlines
                let password = user.password.as_deref().unwrap_or("").replace('\'', "''");
                let user_sql = format!(
                    "DO $crane$\n\
                     BEGIN\n\
                         IF NOT EXISTS (SELECT FROM pg_catalog.pg_roles WHERE rolname = '{}') THEN\n\
                             CREATE ROLE {} WITH PASSWORD '{}' LOGIN;\n\
                         ELSE\n\
                             ALTER ROLE {} WITH PASSWORD '{}';\n\
                         END IF;\n\
                     END $crane$;",
                    user.user, user.user, password, user.user, password
                );
                let tmp_sql = format!("/tmp/crane_user_{}.sql", user.user);
                interactor_clone.create_file(&tmp_sql, &user_sql)?;
                let psql_res = run_psql(&format!("sudo -u postgres psql -f '{}'", tmp_sql));
                let _ = interactor_clone.cmd(&format!("sudo rm -f '{}'", tmp_sql));
                psql_res?;

                for db_ref in &user.databases {
                    let db_name = db_configs
                        .iter()
                        .find(|d| &d.name == db_ref)
                        .map(|d| d.name.as_str())
                        .unwrap_or(db_ref);

                    println!(
                        "\tGranting access for user '{}' to database '{}'...",
                        user.user, db_name
                    );

                    run_psql(&format!(
                        "sudo -u postgres psql -c \"GRANT ALL PRIVILEGES ON DATABASE {} TO {};\"",
                        db_name, user.user
                    ))?;

                    run_psql(&format!(
                        "sudo -u postgres psql -d {} -c \"GRANT ALL ON SCHEMA public TO {};\"",
                        db_name, user.user
                    ))?;
                }
            } else {
                anyhow::bail!("unknown user state: {}", user_state);
            }
        }


        Ok(())
    })
    .await??;

    Ok(())
}
