use crane::config::read_config_toml_file_with_env;

#[test]
fn test_parse_config() {
    let toml_content = r#"
[[nodes]]
name = "vps1"
public_ip = "10.0.0.11"
internal_ip = "10.0.0.11"
ssh_ip = "127.0.0.1"
ssh_port = 2221
roles = ["app", "haproxy", "postgres", "redis"]
ssh_user = "crane"
private_key = "keys/id_ed25519"
sudo_pass = "${SUDO_PASS_VPS1}"
firewall_expose_port_range = ["3000-3100", "8080"]

[app.myapp]
name = "myapp"
deploy_dir = "./examples"
entrypoint = "./myapp"
deploy_user = "deployman"
port_start = 3000
instances = 1

[backup.s3]
enabled = true
bucket = "crane1"
region = "us-east-1"
endpoint = "http://s3:9000"
access_key_id = "${S3_ACCESS_KEY_ID}"
secret_access_key = "${S3_SECRET_ACCESS_KEY}"

[domain]
enabled = true
provider = "cloudflare"
domain_name = "localhost"
token = "${CLOUDFLARE_TOKEN}"
"#;

    let temp_dir = std::env::temp_dir();
    let config_path = temp_dir.join("test_crane_parse.toml");
    let env_path = temp_dir.join("test_crane_parse.env");

    let env_content = "SUDO_PASS_VPS1=pass\nS3_ACCESS_KEY_ID=key\nS3_SECRET_ACCESS_KEY=secret\nCLOUDFLARE_TOKEN=token\n";
    std::fs::write(&config_path, toml_content).unwrap();
    std::fs::write(&env_path, env_content).unwrap();

    let parse_config_result = read_config_toml_file_with_env(&config_path, Some(&env_path));

    let _ = std::fs::remove_file(&config_path);
    let _ = std::fs::remove_file(&env_path);

    assert!(
        parse_config_result.is_ok(),
        "should parse config ok, but got error: {:?}",
        parse_config_result.unwrap_err()
    );

    let config = parse_config_result.unwrap();
    assert_eq!(config.nodes[0].ssh_user, "crane");
    assert_eq!(
        config.nodes[0].firewall_expose_port_range,
        Some(vec![
            crane::config::PortRange { start: 3000, end: 3100 },
            crane::config::PortRange { start: 8080, end: 8080 },
        ])
    );
    assert!(config.domain.unwrap().enabled);
    assert!(config.backup.unwrap().s3.unwrap().enabled);
}

#[test]
fn test_postgres_minimal_default_false() {
    let toml_content = r#"
[[nodes]]
name = "vps1"
public_ip = "10.0.0.11"
internal_ip = "10.0.0.11"
ssh_ip = "127.0.0.1"
ssh_port = 2221
roles = ["app", "postgres"]
ssh_user = "crane"
private_key = "keys/id_ed25519"

[[nodes]]
name = "vps2"
public_ip = "10.0.0.12"
internal_ip = "10.0.0.12"
ssh_ip = "127.0.0.1"
ssh_port = 2222
roles = ["postgres"]
ssh_user = "crane"
private_key = "keys/id_ed25519"

[app.myapp]
name = "myapp"
deploy_dir = "./examples"
entrypoint = "./myapp"
deploy_user = "deployman"
port_start = 3000
instances = 1

[db.postgres]
enabled = true
version = "18"
replica_pass = "secret"
"#;

    let config: crane::config::Config = toml::from_str(toml_content).unwrap();
    let pg = config.db.as_ref().unwrap().postgres.as_ref().unwrap();
    assert!(!pg.minimal, "minimal should default to false when omitted");
    assert!(!crane::config::postgres_is_minimal(&config));
    assert!(crane::config::validate_config(&config).is_ok());
}

#[test]
fn test_postgres_minimal_true_single_node() {
    let toml_content = r#"
[[nodes]]
name = "vps1"
public_ip = "10.0.0.11"
internal_ip = "10.0.0.11"
ssh_ip = "127.0.0.1"
ssh_port = 2221
roles = ["app", "postgres"]
ssh_user = "crane"
private_key = "keys/id_ed25519"

[app.myapp]
name = "myapp"
deploy_dir = "./examples"
entrypoint = "./myapp"
deploy_user = "deployman"
port_start = 3000
instances = 1

[db.postgres]
enabled = true
minimal = true
version = "18"
replica_pass = "secret"
"#;

    let config: crane::config::Config = toml::from_str(toml_content).unwrap();
    let pg = config.db.as_ref().unwrap().postgres.as_ref().unwrap();
    assert!(pg.minimal, "minimal should be parsed as true");
    assert!(crane::config::postgres_is_minimal(&config));
    assert!(crane::config::validate_config(&config).is_ok());
}

#[test]
fn test_postgres_minimal_invalid_multi_node() {
    let toml_content = r#"
[[nodes]]
name = "vps1"
public_ip = "10.0.0.11"
internal_ip = "10.0.0.11"
ssh_ip = "127.0.0.1"
ssh_port = 2221
roles = ["app", "postgres"]
ssh_user = "crane"
private_key = "keys/id_ed25519"

[[nodes]]
name = "vps2"
public_ip = "10.0.0.12"
internal_ip = "10.0.0.12"
ssh_ip = "127.0.0.1"
ssh_port = 2222
roles = ["postgres"]
ssh_user = "crane"
private_key = "keys/id_ed25519"

[app.myapp]
name = "myapp"
deploy_dir = "./examples"
entrypoint = "./myapp"
deploy_user = "deployman"
port_start = 3000
instances = 1

[db.postgres]
enabled = true
minimal = true
version = "18"
replica_pass = "secret"
"#;

    let config: crane::config::Config = toml::from_str(toml_content).unwrap();
    let res = crane::config::validate_config(&config);
    assert!(res.is_err(), "minimal with multiple postgres nodes should fail validation");
    let err = res.unwrap_err().to_string();
    assert!(
        err.contains("minimal mode requires exactly 1 node with role 'postgres'"),
        "error message should explain single-node requirement, got: {}",
        err
    );
}

#[test]
fn test_postgres_minimal_invalid_zero_node() {
    let toml_content = r#"
[[nodes]]
name = "vps1"
public_ip = "10.0.0.11"
internal_ip = "10.0.0.11"
ssh_ip = "127.0.0.1"
ssh_port = 2221
roles = ["app"]
ssh_user = "crane"
private_key = "keys/id_ed25519"

[app.myapp]
name = "myapp"
deploy_dir = "./examples"
entrypoint = "./myapp"
deploy_user = "deployman"
port_start = 3000
instances = 1

[db.postgres]
enabled = true
minimal = true
version = "18"
replica_pass = "secret"
"#;

    let config: crane::config::Config = toml::from_str(toml_content).unwrap();
    let res = crane::config::validate_config(&config);
    assert!(res.is_err(), "minimal with zero postgres nodes should fail validation");
    let err = res.unwrap_err().to_string();
    assert!(
        err.contains("minimal mode requires exactly 1 node with role 'postgres'"),
        "error message should explain single-node requirement, got: {}",
        err
    );
}
