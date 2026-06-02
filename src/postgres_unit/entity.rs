#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct BackupMetadata {
    pub id: String,
    pub date: String,
    pub time: String,
    pub taken_at: Option<String>, // "YYYY-MM-DD HH:MM:SS" combined for PITR validation
    pub backup_type: String,      // "FULL" or "INCR"
    pub base: Option<String>,
    pub local_path: String,
    pub s3_path: String,
    // pub last_executed_sql_time: Option<String>, // "YYYY-MM-DD HH:MM:SS"
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct BackupRegistry {
    pub backups: Vec<BackupMetadata>,
}

#[derive(Debug)]
pub struct PostgresNode {
    pub hostname: String,
    pub address: String,
    pub role: String,
    pub version: String,
    pub status: String,
}

pub struct HAProxyNode {
    pub status: String,
    pub primary: String,
    pub replicas: Vec<String>,
}

pub struct PostgresStatusOutput {
    pub haproxy: HAProxyNode,
    pub postgres: Vec<PostgresNode>,
}
