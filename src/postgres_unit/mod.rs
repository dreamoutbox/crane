pub mod backup;
pub mod backup_metadata;
pub mod backup_registry;
pub mod cron;
pub mod entity;
pub mod haproxy_node;
pub mod helper;
pub mod migration;
pub mod postgres_deployment_mode;
pub mod postgres_node;
pub mod postgres_status_output;
pub mod restore;
pub mod setup;
pub mod setup_minimal;
pub mod setup_postgres_primary;

pub const PYTHON_BACKUP_SCRIPT: &str = include_str!("backup.py");

pub const PYTHON_PARSE_PG_LOG_SCRIPT: &str = include_str!("parse_pg_log.py");
