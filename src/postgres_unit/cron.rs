use crate::{
    config::PostgresBackupSchedule, helper::cron::interval_to_cron,
    postgres_unit::PYTHON_BACKUP_SCRIPT,
    server_interactor::server_interactor_trait::ServerInteractor,
};

pub fn configure_postgres_cron_backup(
    interactor: &dyn ServerInteractor,
    version: &str,
    replica_pass: &str,
    schedule: &Option<PostgresBackupSchedule>,
    s3_config: &Option<crate::s3::S3Config>,
) -> anyhow::Result<()> {
    if let Some(schedule) = schedule {
        // Resolve S3Config
        let s3_config = s3_config.as_ref().ok_or_else(|| {
            anyhow::anyhow!("S3 backup configuration [backup.s3] is missing in crane.toml")
        })?;

        // Ensure directories exist
        interactor.mkdir("/etc/crane")?;
        interactor.mkdir("/opt/crane")?;
        interactor.mkdir("/var/lib/postgresql/backups")?;
        interactor.chown("/var/lib/postgresql/backups", "postgres", "postgres")?;
        interactor.chmod("/var/lib/postgresql/backups", "755")?;

        // Write postgres-backup-config.toml
        let s3_toml_str = format!(
            r#"[pg_backup_cron]
bucket = "{}"
region = "{}"
endpoint = {}
access_key = "{}"
secret_key = "{}"
pg_version = "{}"
replica_pass = "{}"
"#,
            s3_config.bucket,
            s3_config.region,
            s3_config
                .endpoint
                .as_ref()
                .map(|e| format!("\"{}\"", e))
                .unwrap_or_else(|| "null".to_string()),
            s3_config.access_key,
            s3_config.secret_key,
            version,
            replica_pass
        );
        // Write postgres-backup-config.toml directly
        interactor.create_file("/etc/crane/postgres-backup-config.toml", &s3_toml_str)?;
        interactor.chown("/etc/crane/postgres-backup-config.toml", "root", "root")?;
        interactor.chmod("/etc/crane/postgres-backup-config.toml", "600")?;

        // Write postgres-backup.py directly
        interactor.create_file("/opt/crane/postgres-backup.py", PYTHON_BACKUP_SCRIPT)?;
        interactor.chown("/opt/crane/postgres-backup.py", "root", "root")?;
        interactor.chmod("/opt/crane/postgres-backup.py", "755")?;

        // Write cron schedule
        let full_cron = interval_to_cron(&schedule.full_backup_every);
        let incr_cron = interval_to_cron(&schedule.incremental_backup_every);

        let cron_content = format!(
            r#"
# Crane Postgres Backups
{} root python3 /opt/crane/postgres-backup.py full >> /var/log/crane-backup.log 2>&1
{} root python3 /opt/crane/postgres-backup.py incr >> /var/log/crane-backup.log 2>&1
            "#,
            full_cron, incr_cron
        );

        interactor.mkdir("/etc/cron.d/")?;
        interactor.create_file("/etc/cron.d/postgres-backup", &cron_content)?;
        interactor.chown("/etc/cron.d/postgres-backup", "root", "root")?;
        interactor.chmod("/etc/cron.d/postgres-backup", "644")?;
    }

    Ok(())
}
