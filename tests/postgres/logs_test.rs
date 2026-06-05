static POSTGRES_LOGS_COMMANDS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

#[test]
fn test_postgres_logs_command() {
    POSTGRES_LOGS_COMMANDS.lock().unwrap().clear();
    let _config_path = std::path::Path::new("demo/crane.toml");

    let interactor = MockServerInteractorLogsRecorder;
    let result = crane::commands::postgres_logs::run_postgres_logs_wrapper(
        &interactor,
        Some("2026-05-26 09:00:00"),
        None,
        Some("deployman"),
        Some("myapp_db"),
        Some("DELETE"),
    );

    assert!(result.is_ok());

    let recorded = POSTGRES_LOGS_COMMANDS.lock().unwrap().clone();
    assert!(recorded.len() >= 4);
    assert!(recorded[0].contains("id postgres"));
    assert!(recorded[1].contains(
        "SELECT current_setting('data_directory') || '/' || pg_current_logfile('csvlog')"
    ));
    assert!(recorded[2].contains("sudo python3 /tmp/crane_parse_pg_logs.py"));
    assert!(recorded[2].contains("--since '2026-05-26 09:00:00'"));
    assert!(recorded[2].contains("--user 'deployman'"));
    assert!(recorded[2].contains("--db 'myapp_db'"));
    assert!(recorded[2].contains("--sql 'DELETE'"));
    assert!(recorded[3].contains("rm -f /tmp/crane_parse_pg_logs.py"));
}

#[test]
fn test_postgres_logs_command_user_not_exist() {
    POSTGRES_LOGS_COMMANDS.lock().unwrap().clear();
    let _config_path = std::path::Path::new("demo/crane.toml");

    let interactor = MockServerInteractorUserNotExist;
    let result = crane::commands::postgres_logs::run_postgres_logs_wrapper(
        &interactor,
        None,
        None,
        None,
        None,
        None,
    );

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains(
        "PostgreSQL is not installed or the 'postgres' user does not exist on the target node"
    ));
}
