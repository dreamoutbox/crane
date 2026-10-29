/// Metadata record for a single PostgreSQL backup.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct BackupMetadata {
    pub id: String,
    pub date: String,
    pub time: String,
    pub taken_at: Option<String>,
    pub backup_type: String,
    pub base: Option<String>,
    pub local_path: String,
    pub s3_path: String,
    pub label: Option<String>,
}

