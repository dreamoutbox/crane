#[path = "../common/mock_interactor.rs"]
pub mod mock_interactor;

use crane::haproxy_unit::haproxy::build_haproxy_config;

// #[test]
// fn test_haproxy_install() {
//     let interactor = MockInteractor::new(vec![]);
//     let result = interactor.setup_haproxy();
//     assert!(result.is_ok());
// }

#[test]
fn test_haproxy_setup_config() {
    let toml_str = r#"
    [[nodes]]
    name = "vps1"
    public_ip = "127.0.0.1"
    internal_ip = "127.0.0.1"
    ssh_ip = "127.0.0.1"
    ssh_port = 2221
    roles = ["app", "haproxy"]
    ssh_user = "crane"
    private_key = "keys/id_ed25519"
    sudo_pass = "cranepass"

    [domain]
    enabled = true
    provider = "cloudflare"
    domain_name = "example.com"

    [app.myapp]
    name = "myapp"
    deploy_dir = "./demo"
    entrypoint = "./myapp"
    deploy_user = "crane"
    port_start = 3000
    instances = 2
    "#;

    let config: crane::config::Config = toml::from_str(toml_str).unwrap();
    let cfg_content = build_haproxy_config(&config, "/etc/ssl/cert.pem".to_string()).unwrap();

    // Check for HTTP and HTTPS frontend and app backend
    assert!(cfg_content.contains("frontend http_front"));
    assert!(cfg_content.contains("frontend https_front"));
    assert!(cfg_content.contains("backend myapp_backend"));
    assert!(cfg_content.contains("server myapp-port-3000 127.0.0.1:3000 check"));
    assert!(cfg_content.contains("server myapp-port-3001 127.0.0.1:3001 check"));
}

#[test]
fn test_haproxy_domain_disabled_ignores_ssl_cert() {
    let toml_str = r#"
    [[nodes]]
    name = "vps1"
    public_ip = "127.0.0.1"
    internal_ip = "127.0.0.1"
    ssh_ip = "127.0.0.1"
    ssh_port = 2221
    roles = ["app", "haproxy"]
    ssh_user = "crane"
    private_key = "keys/id_ed25519"

    [domain]
    enabled = false
    provider = "cloudflare"
    domain_name = "example.com"
    ssl_cert = "/etc/ssl/domain.pem"

    [app.myapp]
    name = "myapp"
    deploy_dir = "./demo"
    entrypoint = "./myapp"
    deploy_user = "crane"
    port_start = 3000
    instances = 1
    ssl_cert = "/etc/ssl/myapp.pem"
    "#;

    let config: crane::config::Config = toml::from_str(toml_str).unwrap();
    let cfg_content = build_haproxy_config(&config, "/etc/ssl/cert.pem".to_string()).unwrap();

    assert!(!cfg_content.contains("/etc/ssl/domain.pem"));
    assert!(!cfg_content.contains("/etc/ssl/myapp.pem"));
}
