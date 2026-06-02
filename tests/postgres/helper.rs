fn try_connect(user: &str, password: &str, db: &str) -> Result<String, String> {
    let output = Command::new("psql")
        .env("PGPASSWORD", password)
        .args(["-h", "127.0.0.1", "-U", user, "-d", db, "-c", "SELECT 1"])
        .output();

    match output {
        Ok(out) => {
            if out.status.success() {
                Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
            } else {
                Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

fn allow_host_connection(config_path: &std::path::Path) {
    use crane::postgres_unit::helper::postgres_get_primary;

    let config = crane::config::read_config_toml_file(config_path).expect("Failed to load config");
    let primary_node = postgres_get_primary(&config)
        .expect("Failed to get leader node")
        .expect("No active PostgreSQL leader found");
    let primary_interactor = crane::postgres_unit::helper::connect_to_node(&primary_node, &config)
        .expect("Failed to connect to primary node");

    let pg_version = crane::postgres_unit::helper::get_pg_version(&config);
    let pg_hba_path = format!("/etc/postgresql/{}/main/pg_hba.conf", pg_version);
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
}
