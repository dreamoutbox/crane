use crane::config::resolve_placeholders;
use std::collections::HashMap;

mod parse_config_test;

#[test]
fn test_resolve_placeholders_success() {
    let mut env = HashMap::new();
    env.insert("FOO".to_string(), "bar".to_string());
    env.insert("BAZ".to_string(), "qux".to_string());

    // Test resolving from dot_env
    let input = "key = \"${FOO}\" and another_key = \"${BAZ}\"";
    let output = resolve_placeholders(input, &env).unwrap();
    assert_eq!(output, "key = \"bar\" and another_key = \"qux\"");

    // Test resolving from shell env
    unsafe {
        std::env::set_var("SHELL_VAR", "shell_val");
    }
    let input2 = "key = \"${SHELL_VAR}\"";
    let output2 = resolve_placeholders(input2, &env).unwrap();
    assert_eq!(output2, "key = \"shell_val\"");

    // Test shell env overrides dot_env
    env.insert("OVERRIDE_VAR".to_string(), "file_val".to_string());
    unsafe {
        std::env::set_var("OVERRIDE_VAR", "env_val");
    }
    let input3 = "override = \"${OVERRIDE_VAR}\"";
    let output3 = resolve_placeholders(input3, &env).unwrap();
    assert_eq!(output3, "override = \"env_val\"");
}

#[test]
fn test_resolve_placeholders_missing_error() {
    let env = HashMap::new();
    let input = "missing = \"${NOT_DEFINED}\"";
    let res = resolve_placeholders(input, &env);
    assert!(res.is_err());
    let err_msg = res.err().unwrap().to_string();
    assert!(err_msg.contains("Environment variable 'NOT_DEFINED' not found"));
}

#[test]
fn test_resolve_placeholders_unclosed_error() {
    let env = HashMap::new();
    let input = "unclosed = \"${FOO\"";
    let res = resolve_placeholders(input, &env);
    assert!(res.is_err());
    assert!(
        res.err()
            .unwrap()
            .to_string()
            .contains("Unclosed placeholder")
    );
}

#[test]
fn test_port_range_parse() {
    use crane::config::PortRange;

    // Single port
    let single: PortRange = "8080".to_string().try_into().unwrap();
    assert_eq!(single, PortRange { start: 8080, end: 8080 });

    // Range
    let range: PortRange = "3000-3100".to_string().try_into().unwrap();
    assert_eq!(range, PortRange { start: 3000, end: 3100 });

    // Single with whitespace
    let single_ws: PortRange = "  22  ".to_string().try_into().unwrap();
    assert_eq!(single_ws, PortRange { start: 22, end: 22 });

    // Range with whitespace
    let range_ws: PortRange = " 3000 - 3100 ".to_string().try_into().unwrap();
    assert_eq!(range_ws, PortRange { start: 3000, end: 3100 });

    // Inverted range fails
    let inverted: Result<PortRange, _> = "3100-3000".to_string().try_into();
    assert!(inverted.is_err());

    // Port 0 fails
    let zero: Result<PortRange, _> = "0".to_string().try_into();
    assert!(zero.is_err());
    let zero_range: Result<PortRange, _> = "0-100".to_string().try_into();
    assert!(zero_range.is_err());

    // Non-numeric fails
    let non_num: Result<PortRange, _> = "abc".to_string().try_into();
    assert!(non_num.is_err());
}

#[test]
fn test_node_ssh_user_and_expose_ports() {
    let toml_valid = r#"
    [[nodes]]
    name = "vps1"
    public_ip = "127.0.0.1"
    internal_ip = "127.0.0.1"
    ssh_ip = "127.0.0.1"
    ssh_port = 22
    ssh_user = "root"
    roles = ["app"]
    private_key = "key"
    firewall_expose_port_range = ["3000-3100", "22", "80", "8080"]
    [app.myapp]
    name = "myapp"
    deploy_dir = "./"
    entrypoint = "./app"
    deploy_user = "app"
    port_start = 3000
    instances = 1
    "#;

    let parsed: crane::config::Config = toml::from_str(toml_valid).unwrap();
    assert_eq!(parsed.nodes[0].ssh_user, "root");
    let ranges = parsed.nodes[0].firewall_expose_port_range.as_ref().unwrap();
    assert_eq!(ranges.len(), 4);
    assert_eq!(ranges[0], crane::config::PortRange { start: 3000, end: 3100 });
    assert_eq!(ranges[1], crane::config::PortRange { start: 22, end: 22 });
    assert_eq!(ranges[2], crane::config::PortRange { start: 80, end: 80 });
    assert_eq!(ranges[3], crane::config::PortRange { start: 8080, end: 8080 });

    // Old 'user' in [[nodes]] should fail deserialization because ssh_user is required
    let toml_old_user = r#"
    [[nodes]]
    name = "vps1"
    public_ip = "127.0.0.1"
    internal_ip = "127.0.0.1"
    ssh_ip = "127.0.0.1"
    ssh_port = 22
    user = "root"
    roles = ["app"]
    private_key = "key"
    [app.myapp]
    name = "myapp"
    deploy_dir = "./"
    entrypoint = "./app"
    deploy_user = "app"
    port_start = 3000
    instances = 1
    "#;

    let res: Result<crane::config::Config, _> = toml::from_str(toml_old_user);
    assert!(res.is_err(), "Expected failure when 'ssh_user' is missing");
}

#[test]
fn test_domain_enabled_required() {
    let toml_missing_enabled = r#"
    [[nodes]]
    name = "vps1"
    public_ip = "127.0.0.1"
    internal_ip = "127.0.0.1"
    ssh_ip = "127.0.0.1"
    ssh_port = 22
    ssh_user = "root"
    roles = ["app"]
    private_key = "key"
    [domain]
    provider = "cloudflare"
    domain_name = "example.com"
    [app.myapp]
    name = "myapp"
    deploy_dir = "./"
    entrypoint = "./app"
    deploy_user = "app"
    port_start = 3000
    instances = 1
    "#;

    let res: Result<crane::config::Config, _> = toml::from_str(toml_missing_enabled);
    assert!(res.is_err(), "Expected missing 'enabled' in [domain] to fail");
}

#[test]
fn test_backup_enabled_required() {
    let toml_missing_enabled = r#"
    [[nodes]]
    name = "vps1"
    public_ip = "127.0.0.1"
    internal_ip = "127.0.0.1"
    ssh_ip = "127.0.0.1"
    ssh_port = 22
    ssh_user = "root"
    roles = ["app"]
    private_key = "key"
    [backup.s3]
    bucket = "test"
    access_key_id = "ak"
    secret_access_key = "sk"
    [app.myapp]
    name = "myapp"
    deploy_dir = "./"
    entrypoint = "./app"
    deploy_user = "app"
    port_start = 3000
    instances = 1
    "#;

    let res: Result<crane::config::Config, _> = toml::from_str(toml_missing_enabled);
    assert!(res.is_err(), "Expected missing 'enabled' in [backup.s3] to fail");
}

#[test]
fn test_domain_disabled_helpers() {
    let toml_disabled = r#"
    [[nodes]]
    name = "vps1"
    public_ip = "127.0.0.1"
    internal_ip = "127.0.0.1"
    ssh_ip = "127.0.0.1"
    ssh_port = 22
    ssh_user = "root"
    roles = ["app"]
    private_key = "key"
    [domain]
    enabled = false
    provider = "cloudflare"
    domain_name = "example.com"
    [app.myapp]
    name = "myapp"
    domain = "myapp.example.com"
    ssl_cert = "/etc/ssl/myapp.pem"
    deploy_dir = "./"
    entrypoint = "./app"
    deploy_user = "app"
    port_start = 3000
    instances = 1
    "#;

    let config: crane::config::Config = toml::from_str(toml_disabled).unwrap();
    let app = config.app.get("myapp").unwrap();

    assert!(crane::config::enabled_domain(&config).is_none());
    assert_eq!(crane::config::app_domain(&config, app), None);
    assert_eq!(crane::config::app_ssl_cert(&config, app), None);
}
