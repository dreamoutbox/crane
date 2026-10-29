use crate::postgres_unit::backup_metadata::BackupMetadata;

/// Registry holding all known PostgreSQL backups.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct BackupRegistry {
    pub backups: Vec<BackupMetadata>,
}

