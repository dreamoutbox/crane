/// Represents the deployment topology mode of PostgreSQL on target servers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostgresDeploymentMode {
    /// Standalone single-node PostgreSQL instance without Patroni or etcd.
    Minimal,
    /// High-availability clustered PostgreSQL orchestrated by Patroni and etcd.
    Cluster,
    /// PostgreSQL is not yet deployed or configured on the target node.
    Uninstalled,
}

impl PostgresDeploymentMode {
    /// Returns a human-readable description of the deployment mode.
    ///
    /// # Example
    /// ```no_run
    /// use crane::postgres_unit::postgres_deployment_mode::PostgresDeploymentMode;
    ///
    /// let mode = PostgresDeploymentMode::Minimal;
    /// assert_eq!(mode.as_str(), "Minimal (standalone single-node)");
    /// ```
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Minimal => "Minimal (standalone single-node)",
            Self::Cluster => "Cluster (Patroni + etcd HA)",
            Self::Uninstalled => "Uninstalled",
        }
    }
}
