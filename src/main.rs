use clap::{Arg, Command};

fn main() {
    let matches = Command::new("crane")
        .version("0.1.0")
        .about("crane — CLI Deployment Tool")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("deploy")
                .about("Deploy apps to VPS nodes")
                .arg(
                    Arg::new("config")
                        .short('f')
                        .long("config")
                        .value_name("FILE")
                        .help("Sets a custom config file")
                        .default_value("crane.toml"),
                ),
        )
        .subcommand(
            Command::new("postgres")
                .about("Manage PostgreSQL cluster topology")
                .subcommand_required(true)
                .arg_required_else_help(true)
                .arg(
                    Arg::new("config")
                        .short('f')
                        .long("config")
                        .value_name("FILE")
                        .help("Sets a custom config file")
                        .default_value("crane.toml"),
                )
                .subcommand(
                    Command::new("promote")
                        .about("Promote a node to PostgreSQL leader")
                        .arg(
                            Arg::new("node")
                                .required(true)
                                .help("The host/IP of the node to promote"),
                        ),
                )
                .subcommand(
                    Command::new("demote")
                        .about("Demote a node to PostgreSQL follower")
                        .arg(
                            Arg::new("node")
                                .required(true)
                                .help("The host/IP of the node to demote"),
                        ),
                )
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
                        ),
                ),
        )
        .get_matches();

    match matches.subcommand() {
        Some(("deploy", sub_m)) => {
            let config_file = sub_m.get_one::<String>("config").unwrap();
            let config_path = std::path::Path::new(config_file);

            if let Err(e) =
                crane::commands::deploy::run(config_path, crane::server_interactor::get_interactor)
            {
                eprintln!("Deployment failed: {}", e);
                std::process::exit(1);
            }
        }

        Some(("postgres", sub_m)) => {
            let config_file = sub_m.get_one::<String>("config").unwrap();
            let config_path = std::path::Path::new(config_file);

            match sub_m.subcommand() {
                Some(("promote", sub_sub_m)) => {
                    let target_node = sub_sub_m.get_one::<String>("node").unwrap();
                    if let Err(e) = crane::commands::postgres::promote(
                        config_path,
                        target_node,
                        crane::server_interactor::get_interactor,
                    ) {
                        eprintln!("Promotion failed: {}", e);
                        std::process::exit(1);
                    }
                }

                Some(("demote", sub_sub_m)) => {
                    let target_node = sub_sub_m.get_one::<String>("node").unwrap();
                    if let Err(e) = crane::commands::postgres::demote(
                        config_path,
                        target_node,
                        crane::server_interactor::get_interactor,
                    ) {
                        eprintln!("Demotion failed: {}", e);
                        std::process::exit(1);
                    }
                }

                Some(("status", _)) => {
                    if let Err(e) = crane::commands::postgres::status(
                        config_path,
                        crane::server_interactor::get_interactor,
                    ) {
                        eprintln!("Status check failed: {}", e);
                        std::process::exit(1);
                    }
                }

                Some(("backup", sub_sub_m)) => {
                    let backup_type = sub_sub_m.get_one::<String>("type").unwrap();

                    // dbg!(backup_type);
                    if backup_type != "full" && backup_type != "incr" {
                        eprintln!("Backup type must be 'full' or 'incr'");
                        std::process::exit(1);
                    }

                    if let Err(e) = crane::commands::postgres::backup(
                        config_path,
                        backup_type,
                        crane::server_interactor::get_interactor,
                    ) {
                        eprintln!("Backup failed: {}", e);
                        std::process::exit(1);
                    }
                }

                Some(("list", _)) => {
                    if let Err(e) = crane::commands::postgres::list_backups(
                        config_path,
                        crane::server_interactor::get_interactor,
                    ) {
                        eprintln!("Listing backups failed: {}", e);
                        std::process::exit(1);
                    }
                }

                Some(("restore", sub_sub_m)) => {
                    let target_id = sub_sub_m.get_one::<String>("id").unwrap();
                    if let Err(e) = crane::commands::postgres::restore(
                        config_path,
                        target_id,
                        crane::server_interactor::get_interactor,
                    ) {
                        eprintln!("Restore failed: {}", e);
                        std::process::exit(1);
                    }
                }

                _ => unreachable!(),
            }
        }

        _ => unreachable!(),
    }
}
