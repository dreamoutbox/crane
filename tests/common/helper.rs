use std::path::Path;
use std::process::Command;

use crane::{postgres_unit::helper::pg_get_primary, server_interactor::get_server_interactor};

/// Resets the development Docker environment by running `./dev-docker/bootstrap.sh`.
///
/// Ensures the Docker daemon is available and local SSH keys have proper permissions
/// before bootstrapping mock VPS nodes and SeaweedFS S3 storage.
///
/// # Arguments
/// None.
///
/// # Example
/// ```no_run
/// use crate::common_helper::reset_docker_compose;
///
/// # async fn run() {
/// reset_docker_compose().await;
/// # }
/// ```
pub async fn reset_docker_compose() {
    let docker_check = Command::new("docker").arg("info").output();

    let docker_available = match docker_check {
        Ok(output) => output.status.success(),
        Err(_) => false,
    };

    if !docker_available {
        println!(
            "Docker daemon is not running or docker is not installed. Skipping docker-dependent test."
        );
        std::process::exit(0);
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata("dev-ssh-keys/id_ed25519") {
            let mut perms = meta.permissions();
            perms.set_mode(0o600);
            let _ = std::fs::set_permissions("dev-ssh-keys/id_ed25519", perms);
        }
    }

    let status = Command::new("./dev-docker/bootstrap.sh")
        .status()
        .expect("Failed to execute dev-docker/bootstrap.sh");
    assert!(status.success(), "dev-docker/bootstrap.sh failed");
}

/// Compiles the Go demo application binary in the specified directory.
///
/// # Arguments
/// * `dir` - Relative or absolute path to the directory containing Go project files (e.g. `"examples"`).
///
/// # Example
/// ```no_run
/// use crate::common_helper::build_go_demo_app;
///
/// build_go_demo_app("examples");
/// ```
pub fn build_go_demo_app<P: AsRef<Path>>(dir: P) {
    let go_build = Command::new("go")
        .arg("build")
        .current_dir(dir.as_ref())
        .output()
        .expect("failed to run go build");
    assert!(
        go_build.status.success(),
        "go build failed: {}",
        String::from_utf8_lossy(&go_build.stderr)
    );
}

pub fn pg_allow_host_machine(config: &crane::config::Config) {
    let primary_node = pg_get_primary(config)
        .expect("Failed to get leader node")
        .expect("No active PostgreSQL leader found");

    let primary_interactor =
        get_server_interactor(&primary_node.name).expect("Failed to connect to primary node");

    let pg_hba_path = "/etc/postgresql/17/main/pg_hba.conf";
    let add_rule_cmd = format!(
        "echo 'host all all 10.0.0.0/24 scram-sha-256' | sudo tee -a {}",
        pg_hba_path
    );
    primary_interactor
        .cmd(&add_rule_cmd)
        .expect("failed to add pg_hba rule");
    primary_interactor
        .cmd("sudo -u postgres psql -c 'SELECT pg_reload_conf();'")
        .expect("failed to reload pg config");

    primary_interactor
        .firewall_allow_source("10.0.0.0/24")
        .expect("failed to allow subnet in firewall");
    primary_interactor
        .firewall_reload()
        .expect("failed to reload firewall");
}
