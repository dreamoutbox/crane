use crate::{
    postgres_unit::{
        entity::BackupMetadata,
        helper::{
            connect_to_node, get_backups_from_s3, get_pg_version, get_replica_pass,
            postgres_get_primary,
        },
    },
    s3::{get_s3_config, s3_client::RealS3Client},
};

pub fn backup_from_config_wrapper(
    config: &crate::config::Config,
    backup_type: &str,
    label: Option<&str>,
) -> anyhow::Result<BackupMetadata> {
    let s3_config = get_s3_config(&config)?;
    let primary_node = postgres_get_primary(&config)?
        .ok_or_else(|| anyhow::anyhow!("No active PostgreSQL leader found in the cluster."))?;

    let pg_version = get_pg_version(&config);
    let replica_pass = get_replica_pass(&config);

    let s3_client = RealS3Client::new(&s3_config)?;
    let interactor = connect_to_node(&primary_node, &config)?;

    let backups = get_backups_from_s3(&s3_client)?;
    let last_backup = backups.last();

    println!(
        "Starting PostgreSQL backup {} ...",
        backup_type.to_uppercase()
    );

    crate::postgres_unit::backup::postgres_backup(
        &*interactor,
        &s3_client,
        &pg_version,
        backup_type,
        &replica_pass,
        &s3_config.bucket,
        last_backup,
        label,
    )
}

pub fn run_backup_cmd(
    config: &crate::config::Config,
    backup_type: &str,
    label: Option<&str>,
) -> anyhow::Result<()> {
    let meta = backup_from_config_wrapper(config, backup_type, label)?;

    println!("\nBackup successful!\n");
    println!("ID: {}", meta.id);
    println!("Date: {}", meta.date);
    println!("Time: {}", meta.time);
    println!("Type: {}", meta.backup_type);
    if let Some(ref l) = meta.label {
        println!("Label: {}", l);
    }
    println!("LOCAL: {}", meta.local_path);
    println!("S3: {}", meta.s3_path);

    Ok(())
}
