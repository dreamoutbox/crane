use std::path::Path;

#[test]
fn test_status_command() {
    let config_path = Path::new("demo/crane.toml");
    let config = crane::config::read_config_toml_file(config_path).unwrap();
    let result = crane::commands::status::run(&config, config_path, "myapp");

    assert!(result.is_ok());
}
