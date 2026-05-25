use std::cell::RefCell;
use std::collections::HashMap;
use crane::server_interactor::server_interactor_trait::{ServerInteractor, ServiceRegister, UserRegister};
use crane::traefik_unit::install::install_traefik;
use crane::traefik_unit::setup::setup_traefik;
use crane::ssh::CmdOutput;

include!("../common/mock_interactor.rs");

#[test]
fn test_traefik_install_config() {
    let interactor = MockInteractor::new(vec![]);
    let result = install_traefik(&interactor);
    assert!(result.is_ok());

    let files = interactor.files.borrow();
    let yml_content = files.get("/tmp/traefik.yml").expect("traefik.yml should be written to /tmp");
    
    // Check for internal entrypoint
    assert!(yml_content.contains("internal:\n    address: \"127.0.0.1:8080\""));
    // Check that there is no global http redirect
    assert!(!yml_content.contains("redirections:"));
}

#[test]
fn test_traefik_setup_config() {
    let interactor = MockInteractor::new(vec![]);
    let result = setup_traefik(&interactor, "myapp", "myapp.com", 3000, 3002, "/health");
    assert!(result.is_ok());

    let files = interactor.files.borrow();
    let config_content = files.get("/tmp/myapp.toml").expect("myapp.toml should be written to /tmp");

    // Check external secure router
    assert!(config_content.contains("[http.routers.myapp-external]"));
    assert!(config_content.contains("rule = \"Host(`myapp.com`)\""));
    assert!(config_content.contains("entryPoints = [\"websecure\"]"));

    // Check redirect router and middleware
    assert!(config_content.contains("[http.routers.myapp-redirect]"));
    assert!(config_content.contains("entryPoints = [\"web\"]"));
    assert!(config_content.contains("middlewares = [\"myapp-redirect\"]"));
    assert!(config_content.contains("[http.middlewares.myapp-redirect.redirectScheme]"));

    // Check internal router
    assert!(config_content.contains("[http.routers.myapp-internal]"));
    assert!(config_content.contains("rule = \"Host(`myapp`)\""));
    assert!(config_content.contains("entryPoints = [\"internal\", \"web\"]"));
}
