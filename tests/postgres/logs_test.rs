static POSTGRES_LOGS_COMMANDS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

#[test]
fn test_postgres_logs_command() {
    POSTGRES_LOGS_COMMANDS.lock().unwrap().clear();
    let config_path = std::path::Path::new("demo/crane.toml");

    let result = crane::commands::postgres::logs(
        config_path,
        "vps1",
        Some("2026-05-26 09:00:00"),
        None,
        Some("deployman"),
        Some("myapp_db"),
        Some("DELETE"),
        |_ssh| {
            let mock = MockLogsRecorder;
            Ok(Box::new(mock) as Box<dyn ServerInteractor>)
        },
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
    let config_path = std::path::Path::new("demo/crane.toml");

    let result = crane::commands::postgres::logs(
        config_path,
        "vps1",
        None,
        None,
        None,
        None,
        None,
        |_ssh| {
            struct MockUserNotExist;
            impl ServerInteractor for MockUserNotExist {
                fn whoami(&self) -> anyhow::Result<String> {
                    Ok("admin".to_string())
                }
                fn get_os_info(&self) -> anyhow::Result<String> {
                    Ok("Linux".to_string())
                }
                fn cmd(&self, command: &str) -> anyhow::Result<CmdOutput> {
                    if command == "id postgres" {
                        Ok(CmdOutput {
                            stdout: "".to_string(),
                            stderr: "id: 'postgres': no such user".to_string(),
                            exit_code: 1,
                        })
                    } else {
                        Ok(CmdOutput {
                            stdout: "".to_string(),
                            stderr: "".to_string(),
                            exit_code: 0,
                        })
                    }
                }
                fn create_file(&self, _p: &str, _c: &str) -> anyhow::Result<()> {
                    Ok(())
                }
                fn read_file(&self, _p: &str) -> anyhow::Result<String> {
                    Ok("".to_string())
                }
                fn upload(&self, _l: &str, _r: &str) -> anyhow::Result<()> {
                    Ok(())
                }
                fn download(&self, _l: &str, _r: &str) -> anyhow::Result<()> {
                    Ok(())
                }
                fn chmod(&self, _p: &str, _perm: &str) -> anyhow::Result<()> {
                    Ok(())
                }
                fn chown(&self, _p: &str, _u: &str, _g: &str) -> anyhow::Result<()> {
                    Ok(())
                }
                fn mkdir(&self, _p: &str, _u: &str, _g: &str) -> anyhow::Result<()> {
                    Ok(())
                }
                fn ls(&self, _p: &str) -> anyhow::Result<Vec<String>> {
                    Ok(vec![])
                }
                fn install_dependencies(&self, _d: Vec<String>) -> anyhow::Result<()> {
                    Ok(())
                }
                fn register_service(&self, _s: ServiceRegister) -> anyhow::Result<()> {
                    Ok(())
                }
                fn restart_service(&self, _s: &str) -> anyhow::Result<()> {
                    Ok(())
                }
                fn stop_service(&self, _s: &str) -> anyhow::Result<()> {
                    Ok(())
                }
                fn start_service(&self, _s: &str) -> anyhow::Result<()> {
                    Ok(())
                }
                fn status_service(&self, _s: &str) -> anyhow::Result<()> {
                    Ok(())
                }
                fn delete_service(&self, _s: &str) -> anyhow::Result<()> {
                    Ok(())
                }
                fn create_user(&self, _u: UserRegister) -> anyhow::Result<()> {
                    Ok(())
                }
                fn delete_user(&self, _u: &str) -> anyhow::Result<()> {
                    Ok(())
                }
                fn add_user_to_groups(&self, _u: &str, _g: Vec<String>) -> anyhow::Result<()> {
                    Ok(())
                }
                fn remove_user_from_groups(&self, _u: &str, _g: Vec<String>) -> anyhow::Result<()> {
                    Ok(())
                }
                fn list_users(&self) -> anyhow::Result<Vec<String>> {
                    Ok(vec![])
                }
            }
            Ok(Box::new(MockUserNotExist) as Box<dyn ServerInteractor>)
        },
    );

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains(
        "PostgreSQL is not installed or the 'postgres' user does not exist on node 'vps1'"
    ));
}

struct MockLogsRecorder;

impl ServerInteractor for MockLogsRecorder {
    fn whoami(&self) -> anyhow::Result<String> {
        Ok("admin".to_string())
    }
    fn get_os_info(&self) -> anyhow::Result<String> {
        Ok("Linux".to_string())
    }
    fn cmd(&self, command: &str) -> anyhow::Result<CmdOutput> {
        POSTGRES_LOGS_COMMANDS
            .lock()
            .unwrap()
            .push(command.to_string());
        let stdout = if command.contains("pg_current_logfile") {
            "/var/lib/postgresql/17/main/log/postgresql-2026-05-26.csv".to_string()
        } else {
            "dummy log line".to_string()
        };
        Ok(CmdOutput {
            stdout,
            stderr: "".to_string(),
            exit_code: 0,
        })
    }
    fn create_file(&self, _p: &str, _c: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn read_file(&self, _p: &str) -> anyhow::Result<String> {
        Ok("".to_string())
    }
    fn upload(&self, _l: &str, _r: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn download(&self, _l: &str, _r: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn chmod(&self, _p: &str, _perm: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn chown(&self, _p: &str, _u: &str, _g: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn mkdir(&self, _p: &str, _u: &str, _g: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn ls(&self, _p: &str) -> anyhow::Result<Vec<String>> {
        Ok(vec![])
    }
    fn install_dependencies(&self, _d: Vec<String>) -> anyhow::Result<()> {
        Ok(())
    }
    fn register_service(&self, _s: ServiceRegister) -> anyhow::Result<()> {
        Ok(())
    }
    fn restart_service(&self, _s: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn stop_service(&self, _s: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn start_service(&self, _s: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn status_service(&self, _s: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn delete_service(&self, _s: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn create_user(&self, _u: UserRegister) -> anyhow::Result<()> {
        Ok(())
    }
    fn delete_user(&self, _u: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn add_user_to_groups(&self, _u: &str, _g: Vec<String>) -> anyhow::Result<()> {
        Ok(())
    }
    fn remove_user_from_groups(&self, _u: &str, _g: Vec<String>) -> anyhow::Result<()> {
        Ok(())
    }
    fn list_users(&self) -> anyhow::Result<Vec<String>> {
        Ok(vec![])
    }
}
