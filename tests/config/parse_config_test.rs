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
