use crate::postgres_unit::entity::BackupMetadata;

pub fn backup_from_config_wrapper(
    config: &crate::config::Config,
    backup_type: &str,
    label: Option<&str>,
) -> anyhow::Result<BackupMetadata> {
    println!(
        "Starting PostgreSQL backup {} ...",
        backup_type.to_uppercase()
    );

    crate::postgres_unit::backup::postgres_backup(config, backup_type, label)
}

pub fn run_postgres_backup_cmd(
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
