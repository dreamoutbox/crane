use std::path::Path;

#[test]
fn test_status_command() {
    let config_path = Path::new("demo/crane.toml");
    let result = crane::commands::status::run(config_path, "myapp");

    assert!(result.is_ok());
}
