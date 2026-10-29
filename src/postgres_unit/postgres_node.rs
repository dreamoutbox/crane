use crate::config::NodeConfig;

/// Represents the status and role of an individual PostgreSQL node.
#[derive(Debug, Clone)]
pub struct PostgresNode {
    pub node: NodeConfig,
    pub status: String,
    pub role: String,
    pub version: String,
}

