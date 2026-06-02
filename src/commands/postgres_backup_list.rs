use crate::postgres_unit::entity::BackupMetadata;
use crate::postgres_unit::helper::get_backups_from_s3;
use crate::s3::get_s3_config;
use crate::s3::s3_client::RealS3Client;

pub fn list_postgres_backups_wrapper(
    config: &crate::config::Config,
) -> anyhow::Result<Vec<BackupMetadata>> {
    let s3_config = get_s3_config(&config)?;
    let s3_client = RealS3Client::new(&s3_config)?;

    let backups = get_backups_from_s3(&s3_client)?;

    Ok(backups)
}

pub fn run_postgres_backup_list_cmd(config: &crate::config::Config) -> anyhow::Result<()> {
    let backups = list_postgres_backups_wrapper(config)?;

    if backups.is_empty() {
        println!("No backups found in cluster.");
        return Ok(());
    }

    for (idx, backup) in backups.iter().enumerate() {
        if idx > 0 {
            println!();
        }

        println!("ID: {}", backup.id);
        println!("Date: {}", backup.date);
        println!("Time: {}", backup.time);
        println!("Type: {}", backup.backup_type);
        println!(
            "Label: {}",
            backup.label.as_ref().unwrap_or(&"Unnamed".to_string())
        );
        println!("LOCAL: {}", backup.local_path);
        println!("S3: {}", backup.s3_path);
    }

    Ok(())
}
