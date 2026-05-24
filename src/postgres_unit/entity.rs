#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct BackupMetadata {
    pub id: String,
    pub date: String,
    pub time: String,
    pub backup_type: String, // "FULL" or "INCR"
    pub base: Option<String>,
    pub local_path: String,
    pub s3_path: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct BackupRegistry {
    pub backups: Vec<BackupMetadata>,
}
