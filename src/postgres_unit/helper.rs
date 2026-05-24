use crate::config;

pub fn get_pg_version(config: &config::Config) -> String {
    config
        .db
        .as_ref()
        .and_then(|db| db.postgres.as_ref())
        .and_then(|pg| pg.get("version"))
        .and_then(|val| val.as_str())
        .unwrap_or("16")
        .to_string()
}

pub fn get_replica_pass(dot_env: &std::collections::HashMap<String, String>) -> String {
    dot_env
        .get("POSTGRES_PASSWORD")
        .cloned()
        .unwrap_or_else(|| "repl_password".to_string())
}
