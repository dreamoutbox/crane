pub mod backup;
pub mod demote;
pub mod entity;
pub mod helper;
pub mod install;
pub mod patroni;
pub mod restore;
pub mod setup;
pub mod setup_postgres_primary;

pub const PYTHON_BACKUP_SCRIPT: &str = include_str!("backup.py");

pub const PYTHON_PARSE_PG_LOG_SCRIPT: &str = include_str!("parse_pg_log.py");
