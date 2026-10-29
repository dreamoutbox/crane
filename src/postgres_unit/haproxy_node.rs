/// Represents HAProxy frontend/backend status and target endpoints for PostgreSQL.
#[derive(Debug, Clone)]
pub struct HAProxyNode {
    pub status: String,
    pub primary: String,
    pub replicas: Vec<String>,
}

