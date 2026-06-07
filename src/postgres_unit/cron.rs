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
        interactor.cmd("sudo mkdir -p /etc/crane /opt/crane /var/lib/postgresql/backups")?;
        interactor.cmd("sudo chown postgres:postgres /var/lib/postgresql/backups")?;
        interactor.cmd("sudo chmod 755 /var/lib/postgresql/backups")?;

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
        interactor.cmd("sudo chown root:root /etc/crane/postgres-backup-config.toml")?;
        interactor.cmd("sudo chmod 600 /etc/crane/postgres-backup-config.toml")?;

        // Write postgres-backup.py directly
        interactor.create_file("/opt/crane/postgres-backup.py", PYTHON_BACKUP_SCRIPT)?;
        interactor.cmd("sudo chown root:root /opt/crane/postgres-backup.py")?;
        interactor.cmd("sudo chmod 755 /opt/crane/postgres-backup.py")?;

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
        interactor.create_file("/etc/cron.d/postgres-backup", &cron_content)?;
        interactor.cmd("sudo chown root:root /etc/cron.d/postgres-backup")?;
        interactor.cmd("sudo chmod 644 /etc/cron.d/postgres-backup")?;
    }

    Ok(())
}
