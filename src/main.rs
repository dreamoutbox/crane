use clap::{Arg, Command};

fn main() {
    let matches = Command::new("crane")
        .version("0.1.0")
        .about("crane — CLI Deployment Tool")
        .subcommand_required(true)
        .arg_required_else_help(true)
        // .subcommand(Command::new("mvptest").about("Connect to vps1 and execute whoami"))
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
                _ => unreachable!(),
            }
        }

        _ => unreachable!(),
    }
}
