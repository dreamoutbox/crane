use crane::server_interactor::server_interactor_trait::ServerInteractor;
use std::process::Command;

pub fn try_connect(user: &str, password: &str, db: &str) -> Result<String, String> {
    let output = Command::new("psql")
        .env("PGPASSWORD", password)
        .args(["-h", "127.0.0.1", "-U", user, "-d", db, "-c", "SELECT 1"])
        .output();

    match output {
        Ok(out) => {
            if out.status.success() {
                Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
            } else {
                Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

pub fn run_sql(interactor: &dyn ServerInteractor, sql: &str) -> String {
    let cmd = format!("sudo -u postgres psql -d mydb -t -A -c {:?}", sql);
    let out = interactor.cmd(&cmd).expect("SQL execution failed");
    assert_eq!(out.exit_code, 0, "SQL failed: {}", out.stderr);
    out.stdout.trim().to_string()
}
