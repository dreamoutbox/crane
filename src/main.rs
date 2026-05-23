use clap::Command;
use crane::helper::server::get_server_distro;
use crane::server_interactor::SSHSession;
use crane::server_interactor::debian::DebianInteractor;
use crane::server_interactor::server_interactor_trait::ServerInteractor;

fn main() {
    let matches = Command::new("crane")
        .version("0.1.0")
        .about("crane — CLI Deployment Tool")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(Command::new("mvptest").about("Connect to vps1 and execute whoami"))
        .get_matches();

    match matches.subcommand() {
        Some(("mvptest", _)) => {
            // Instantiate SSHSession for vps1 (MVP defaults)
            let ssh = SSHSession::new("vps1".to_string(), "admin".to_string(), "".to_string());

            // Get server distribution
            let distro = match get_server_distro(&ssh) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("Error detecting server distribution: {}", e);
                    std::process::exit(1);
                }
            };

            // Match distro to interactor
            let interactor: Box<dyn ServerInteractor> = match distro.as_str() {
                "debian" | "ubuntu" => Box::new(DebianInteractor::new(ssh)),
                other => {
                    eprintln!(
                        "Unsupported server distribution: {}. Only Debian and Ubuntu are supported.",
                        other
                    );
                    std::process::exit(1);
                }
            };

            // Execute mvptest subcommand using the interactor
            if let Err(e) = crane::commands::mvptest::run(interactor.as_ref()) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        _ => unreachable!(),
    }
}
