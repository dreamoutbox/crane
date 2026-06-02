pub mod backup;
pub mod demote;
pub mod entity;
pub mod helper;
pub mod install;
pub mod patroni;
pub mod python_parse_pg_log_script;
pub mod restore;
pub mod setup;

pub const PYTHON_BACKUP_SCRIPT: &str = include_str!("python_backup_script.py");
