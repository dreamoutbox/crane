use std::path::Path;
use std::sync::Mutex;

static TEST_MUTEX: Mutex<()> = Mutex::new(());

#[test]
fn test_logs_command_single_instance() {
    let _guard = TEST_MUTEX.lock().unwrap();
    let config_path = Path::new("demo/crane.toml");
    let config = crane::config::read_config_toml_file(config_path).unwrap();

    let result = crane::commands::logs::run(
        config,
        "myapp@1",
        50,
        Some("1h ago"),
        None,
        true,
        false, // follow
        false, // no_app_instance_id
    );

    assert!(result.is_ok());
}

#[test]
fn test_logs_command_all_instances() {
    let _guard = TEST_MUTEX.lock().unwrap();
    let config_path = Path::new("demo/crane.toml");
    let config = crane::config::read_config_toml_file(config_path).unwrap();

    let result = crane::commands::logs::run(
        config,
        "myapp",
        100,
        None,
        Some("2026-05-26 12:00:00"),
        false, // timestamps
        false, // follow
        true,  // no_app_instance_id
    );

    assert!(result.is_ok());
}

#[test]
fn test_logs_command_invalid_instance() {
    let _guard = TEST_MUTEX.lock().unwrap();
    let config_path = Path::new("demo/crane.toml");
    let config = crane::config::read_config_toml_file(config_path).unwrap();
    let result =
        crane::commands::logs::run(config, "myapp@99", 100, None, None, false, false, false);

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("Instance ID 99 is invalid. Total instances: 3"));
}
