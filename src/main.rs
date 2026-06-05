use clap::{Arg, Command};
use crane::{
    commands::postgres_backup_list::run_postgres_backup_list_cmd, config::read_config_toml_file,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let matches = Command::new("crane")
        .version("0.1.0")
        .about("crane — CLI Deployment Tool")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .arg(
            Arg::new("config")
                .short('f')
                .long("config")
                .value_name("FILE")
                .help("Sets a custom config file")
                .default_value("crane.toml")
                .global(true),
        )
        .subcommand(
            Command::new("deploy")
                .about("Deploy apps to VPS nodes")
                .arg(
                    Arg::new("no-dns-update")
                        .long("no-dns-update")
                        .action(clap::ArgAction::SetTrue)
                        .help("Skip updating Cloudflare DNS records"),
                ),
        )
        .subcommand(
            Command::new("dns")
                .about("Manage DNS records")
                .subcommand_required(true)
                .arg_required_else_help(true)
                .subcommand(
                    Command::new("update")
                        .about("Update DNS A records for apps based on node IPs")
                        .arg(Arg::new("app").help("The name of the application to update DNS for")),
                ),
        )
        .subcommand(
            Command::new("postgres")
                .alias("pg")
                .about("Manage PostgreSQL cluster topology")
                .subcommand_required(true)
                .arg_required_else_help(true)
                .subcommand(
                    Command::new("promote")
                        .about("Promote a node to PostgreSQL leader")
                        .arg(
                            Arg::new("node")
                                .required(true)
                                .help("The host/IP of the node to promote"),
                        ),
                )
                // .subcommand(
                //     Command::new("demote")
                //         .about("Demote a node to PostgreSQL follower")
                //         .arg(
                //             Arg::new("node")
                //                 .required(true)
                //                 .help("The host/IP of the node to demote"),
                //         ),
                // )
                .subcommand(
                    Command::new("status").about("Get the status of the PostgreSQL cluster"),
                )
                .subcommand(
                    Command::new("backup")
                        .about("Backup PostgreSQL cluster in full or incremental mode")
                        .arg(
                            Arg::new("type")
                                .required(true)
                                .help("Backup type: 'full' or 'incr'"),
                        )
                        .arg(
                            Arg::new("label")
                                .short('l')
                                .long("label")
                                .value_name("LABEL")
                                .help("Optional label name for the backup"),
                        ),
                )
                .subcommand(
                    Command::new("reset")
                        .about("Reset PostgreSQL cluster by stopping Patroni, clearing the data directory and starting Patroni again")
                        .arg(
                            Arg::new("force")
                                .long("force")
                                .action(clap::ArgAction::SetTrue)
                                .help("Force reset without prompting for confirmation"),
                        ),
                )
                .subcommand(Command::new("list").about("List available backups in the cluster"))
                .subcommand(
                    Command::new("restore")
                        .about("Restore PostgreSQL from a backup ID")
                        .arg(
                            Arg::new("id")
                                .required(true)
                                .help("The ID of the backup to restore"),
                        )
                        .arg(
                            Arg::new("base")
                                .long("base")
                                .value_name("BASE_ID")
                                .help("Override root of the restore chain for incremental backups"),
                        )
                        .arg(
                            Arg::new("pitr")
                                .long("pitr")
                                .value_name("TIME")
                                .help("Point-in-time recovery target (YYYY-MM-DD HH:MM:SS, UTC)"),
                        ),
                )
                .subcommand(
                    Command::new("logs")
                        .about("Get the logs of PostgreSQL from the primary node")
                        .arg(Arg::new("since").long("since").value_name("TIME").help(
                            "Filter logs starting from a specific time (YYYY-MM-DD HH:MM:SS)",
                        ))
                        .arg(
                            Arg::new("until").long("until").value_name("TIME").help(
                                "Filter logs ending at a specific time (YYYY-MM-DD HH:MM:SS)",
                            ),
                        )
                        .arg(
                            Arg::new("user")
                                .long("user")
                                .value_name("USER")
                                .help("Filter logs by database user"),
                        )
                        .arg(
                            Arg::new("db")
                                .long("db")
                                .value_name("DB")
                                .help("Filter logs by database name"),
                        )
                        .arg(
                            Arg::new("sql")
                                .long("sql")
                                .value_name("SQL")
                                .help("Filter logs by executed SQL statement pattern"),
                        ),
                ),
        )
        .subcommand(
            Command::new("status")
                .about("Get the status of an application")
                .arg(
                    Arg::new("app")
                        .required(true)
                        .help("The name of the application to check"),
                ),
        )
        .subcommand(
            Command::new("logs")
                .about("Get logs of an application")
                .arg(
                    Arg::new("app")
                        .required(true)
                        .help("The name of the application or app@instance_id"),
                )
                .arg(
                    Arg::new("lines")
                        .short('l')
                        .long("lines")
                        .value_name("LINES")
                        .help("Number of lines to show")
                        .default_value("100")
                        .value_parser(clap::value_parser!(u32)),
                )
                .arg(
                    Arg::new("since")
                        .long("since")
                        .value_name("TIME")
                        .help("Show logs since a relative or absolute time"),
                )
                .arg(
                    Arg::new("until")
                        .long("until")
                        .value_name("TIME")
                        .help("Show logs until a relative or absolute time"),
                )
                .arg(
                    Arg::new("timestamps")
                        .short('t')
                        .long("timestamps")
                        .action(clap::ArgAction::SetTrue)
                        .help("Show timestamps"),
                )
                .arg(
                    Arg::new("follow")
                        // .short('fl')
                        .long("follow")
                        .action(clap::ArgAction::SetTrue)
                        .help("Follow the logs"),
                )
                .arg(
                    Arg::new("no-app-instance-id")
                        .long("no-app-instance-id")
                        .action(clap::ArgAction::SetTrue)
                        .help("Hide the [app@instance_id] prefix on log lines"),
                ),
        )
        .get_matches();

    let config_file = matches.get_one::<String>("config").unwrap();
    let config_path = std::path::Path::new(config_file);
    let config = read_config_toml_file(config_path)?;

    match matches.subcommand() {
        Some(("deploy", sub_m)) => {
            let no_dns_update = sub_m.get_flag("no-dns-update");

            if let Err(e) =
                crane::commands::deploy::run_deploy_command(&config, config_path, no_dns_update)
                    .await
            {
                eprintln!("Deployment failed: {}", e);
                std::process::exit(1);
            }
        }

        Some(("postgres", sub_m)) => {
            match sub_m.subcommand() {
                Some(("promote", sub_sub_m)) => {
                    let target_node = sub_sub_m.get_one::<String>("node").unwrap();
                    if let Err(e) = crane::commands::postgres_promote::run_postgres_promote_cmd(
                        &config,
                        target_node,
                    ) {
                        eprintln!("Promotion failed: {}", e);
                        std::process::exit(1);
                    }
                }

                // Some(("demote", sub_sub_m)) => {
                //     let target_node = sub_sub_m.get_one::<String>("node").unwrap();
                //     if let Err(e) = crane::commands::postgres_demote::run_postgres_demote_cmd(
                //         &config,
                //         target_node,
                //     ) {
                //         eprintln!("Demotion failed: {}", e);
                //         std::process::exit(1);
                //     }
                // }
                Some(("status", _)) => {
                    if let Err(e) =
                        crane::commands::postgres_status::run_postgres_status_command(&config).await
                    {
                        eprintln!("Status check failed: {}", e);
                        std::process::exit(1);
                    }
                }

                Some(("backup", sub_sub_m)) => {
                    let backup_type = sub_sub_m.get_one::<String>("type").unwrap();
                    let label = sub_sub_m.get_one::<String>("label").map(|s| s.as_str());

                    // dbg!(backup_type);
                    if backup_type != "full" && backup_type != "incr" {
                        eprintln!("Backup type must be 'full' or 'incr'");
                        std::process::exit(1);
                    }

                    if let Err(e) = crane::commands::postgres_backup::run_postgres_backup_cmd(
                        &config,
                        backup_type,
                        label,
                    ) {
                        eprintln!("Backup failed: {}", e);
                        std::process::exit(1);
                    }
                }

                Some(("reset", sub_sub_m)) => {
                    let force = sub_sub_m.get_flag("force");
                    if let Err(e) =
                        crane::commands::postgres_reset::run_postgres_reset_cmd(&config, force)
                    {
                        eprintln!("Reset failed: {}", e);
                        std::process::exit(1);
                    }
                }

                Some(("list", _)) => {
                    if let Err(e) = run_postgres_backup_list_cmd(&config) {
                        eprintln!("Listing backups failed: {}", e);
                        std::process::exit(1);
                    }
                }

                Some(("restore", sub_sub_m)) => {
                    let target_id = sub_sub_m.get_one::<String>("id").unwrap();
                    let base_id = sub_sub_m.get_one::<String>("base").map(|s| s.as_str());
                    let pitr_time = sub_sub_m.get_one::<String>("pitr").map(|s| s.as_str());
                    if let Err(e) = crane::commands::postgres_restore::run_postgres_restore_cmd(
                        &config, target_id, base_id, pitr_time,
                    )
                    .await
                    {
                        eprintln!("Restore failed: {}", e);
                        std::process::exit(1);
                    }
                }

                Some(("logs", sub_sub_m)) => {
                    let since = sub_sub_m.get_one::<String>("since").map(|s| s.as_str());
                    let until = sub_sub_m.get_one::<String>("until").map(|s| s.as_str());
                    let user = sub_sub_m.get_one::<String>("user").map(|s| s.as_str());
                    let db = sub_sub_m.get_one::<String>("db").map(|s| s.as_str());
                    let sql = sub_sub_m.get_one::<String>("sql").map(|s| s.as_str());

                    if let Err(e) = crane::commands::postgres_logs::run_postgres_logs_cmd(
                        &config, since, until, user, db, sql,
                    ) {
                        eprintln!("Logs failed: {}", e);
                        std::process::exit(1);
                    }
                }

                _ => unreachable!(),
            }
        }

        Some(("status", sub_m)) => {
            let app_name = sub_m.get_one::<String>("app").unwrap();
            if let Err(e) = crane::commands::status::run_status_command(&config, app_name) {
                eprintln!("Status check failed: {}", e);
                std::process::exit(1);
            }
        }

        Some(("logs", sub_m)) => {
            let app_target = sub_m.get_one::<String>("app").unwrap();

            let lines = *sub_m.get_one::<u32>("lines").unwrap();
            let since = sub_m.get_one::<String>("since").map(|s| s.as_str());
            let until = sub_m.get_one::<String>("until").map(|s| s.as_str());
            let show_timestamps = sub_m.get_flag("timestamps");
            let follow = sub_m.get_flag("follow");
            let no_app_instance_id = sub_m.get_flag("no-app-instance-id");

            if let Err(e) = crane::commands::logs::run_logs_cmd(
                &config,
                app_target,
                lines,
                since,
                until,
                show_timestamps,
                follow,
                no_app_instance_id,
            ) {
                eprintln!("Failed to get logs: {}", e);
                std::process::exit(1);
            }
        }

        Some(("dns", sub_m)) => match sub_m.subcommand() {
            Some(("update", sub_sub_m)) => {
                let app_name = sub_sub_m.get_one::<String>("app").map(|s| s.as_str());

                if let Err(e) = crane::cloudflare_unit::setup::update_dns(&config, app_name).await {
                    eprintln!("DNS update failed: {}", e);
                    std::process::exit(1);
                }
            }
            _ => unreachable!(),
        },

        _ => unreachable!(),
    };

    Ok(())
}
