use std::path::Path;

use crate::{
    config,
    postgres_unit::{
        entity::BackupMetadata,
        helper::{
            connect_to_node, get_backups_from_s3, get_pg_version, get_replica_pass,
            postgres_get_leader,
        },
    },
    s3::{get_s3_config, s3_client::RealS3Client},
};

pub fn run_postgres_backup_cmd(
    config_path: &Path,
    backup_type: &str,
) -> anyhow::Result<BackupMetadata> {
    let config = config::load_config(config_path)?;
    let config_dir = config_path.parent().unwrap_or(Path::new("."));
    let env_path = config_dir.join(".env");
    let dot_env = config::load_env_file(&env_path).unwrap_or_default();

    let s3_config = get_s3_config(&config, &dot_env)?;
    let primary_node = postgres_get_leader(&config)?
        .ok_or_else(|| anyhow::anyhow!("No active PostgreSQL leader found in the cluster."))?;

    let pg_version = get_pg_version(&config);
    let replica_pass = get_replica_pass(&dot_env);

    let s3_client = RealS3Client::new(&s3_config)?;
    let interactor = connect_to_node(&primary_node, &config)?;

    let backups = get_backups_from_s3(&s3_client)?;
    let last_backup = backups.last();

    println!(
        "Starting PostgreSQL {} backup...",
        backup_type.to_uppercase()
    );

    crate::postgres_unit::backup::run_backup(
        &*interactor,
        &s3_client,
        &pg_version,
        backup_type,
        &replica_pass,
        &s3_config.bucket,
        last_backup,
    )
}

pub fn run_backup_cmd(config_path: &Path, backup_type: &str) -> anyhow::Result<()> {
    let meta = run_postgres_backup_cmd(config_path, backup_type)?;

    println!("\nBackup successful!\n");
    println!("ID: {}", meta.id);
    println!("Date: {}", meta.date);
    println!("Time: {}", meta.time);
    println!("Type: {}", meta.backup_type);
    println!("LOCAL: {}", meta.local_path);
    println!("S3: {}", meta.s3_path);

    Ok(())
}
