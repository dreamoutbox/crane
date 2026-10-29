use crate::postgres_unit::{
    haproxy_node::HAProxyNode, postgres_deployment_mode::PostgresDeploymentMode,
    postgres_node::PostgresNode,
};

/// Represents the overall status report of the PostgreSQL deployment,
/// including deployment mode, any topology mismatch warnings, HAProxy status,
/// and per-node health and role information.
#[derive(Debug)]
pub struct PostgresStatusOutput {
    pub mode: PostgresDeploymentMode,
    pub warning: Option<String>,
    pub haproxy: HAProxyNode,
    pub postgres: Vec<PostgresNode>,
}

