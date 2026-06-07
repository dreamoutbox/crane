use crane::haproxy_unit::haproxy::{install_haproxy, setup_haproxy_unified};

#[path = "../common/mock_interactor.rs"]
pub mod mock_interactor;

use mock_interactor::MockInteractor;

#[test]
fn test_haproxy_install() {
    let interactor = MockInteractor::new(vec![]);
    let result = install_haproxy(&interactor);
    assert!(result.is_ok());
}

#[test]
fn test_haproxy_setup_config() {
    let toml_str = r#"
    [[nodes]]
    name = "vps1"
    host = "localhost"
    public_ip = "127.0.0.1"
    internal_ip = "127.0.0.1"
    port = 2221
    roles = ["app", "haproxy"]
    user = "crane"
    private_key = "keys/id_ed25519"
    sudo_pass = "cranepass"

    [domain]
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
    let interactor = MockInteractor::new(vec![]);

    let node = &config.nodes[0];
    let result = setup_haproxy_unified(&interactor, &config, node, Some("myapp"), Some(3002));
    assert!(result.is_ok());

    let files = interactor.files.borrow();
    let cfg_content = files
        .get("/etc/haproxy/haproxy.cfg")
        .expect("haproxy.cfg should be written to /etc/haproxy");

    // Check for HTTP and HTTPS frontend and app backend
    assert!(cfg_content.contains("frontend http_front"));
    assert!(cfg_content.contains("frontend https_front"));
    assert!(cfg_content.contains("backend myapp_backend"));
    assert!(cfg_content.contains("server myapp-port-3000 127.0.0.1:3000 check"));
    assert!(cfg_content.contains("server myapp-port-3001 127.0.0.1:3001 check"));
}
