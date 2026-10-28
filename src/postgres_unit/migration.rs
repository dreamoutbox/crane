use crate::{
    config::{self, postgres_is_minimal},
    helper::config::config_get_nodes,
    postgres_unit::{
        helper::{backup_pg_dir, get_pg_version, pg_get_primary},
        postgres_deployment_mode::PostgresDeploymentMode,
        setup::setup_postgres_cluster,
        setup_minimal::setup_postgres_minimal,
    },
    server_interactor::{get_server_interactor, server_interactor_trait::ServerInteractor},
};

/// Inspects a server node to determine its current PostgreSQL deployment topology.
///
/// # Arguments
/// * `interactor` - The server interactor connected to the target node
/// * `pg_version` - Configured PostgreSQL major version
///
/// # Returns
/// `PostgresDeploymentMode` indicating Cluster, Minimal, or Uninstalled.
///
/// # Example
/// ```no_run
/// use crane::postgres_unit::migration::detect_node_postgres_mode;
/// use crane::server_interactor::server_interactor_trait::ServerInteractor;
///
/// # fn check(interactor: &dyn ServerInteractor) {
/// let mode = detect_node_postgres_mode(interactor, "17");
/// println!("Current mode: {}", mode.as_str());
/// # }
/// ```
pub fn detect_node_postgres_mode(
    interactor: &dyn ServerInteractor,
    pg_version: &str,
) -> PostgresDeploymentMode {
    let patroni_config_path = interactor.server_paths().patroni_config_path;
    let patroni_cfg_exists = interactor.exists(&patroni_config_path).unwrap_or(false);
    let patroni_running = interactor
        .check_http_status("http://127.0.0.1:8008/health")
        .map(|code| code == 200 || code == 503)
        .unwrap_or(false);

    if patroni_cfg_exists || patroni_running {
        return PostgresDeploymentMode::Cluster;
    }

    if let Ok(res) = interactor.psql(Some("SELECT 1;"), None, None, true) {
        if res.stdout.trim() == "1" {
            return PostgresDeploymentMode::Minimal;
        }
    }

    let debian_data = format!("/var/lib/postgresql/{}/main/PG_VERSION", pg_version);
    let rhel_data = format!("/var/lib/pgsql/{}/data/PG_VERSION", pg_version);
    if interactor.exists(&debian_data).unwrap_or(false)
        || interactor.exists(&rhel_data).unwrap_or(false)
    {
        return PostgresDeploymentMode::Minimal;
    }

    PostgresDeploymentMode::Uninstalled
}

/// Automatically detects if `minimal` setting changed and executes data migration between modes.
///
/// # Arguments
/// * `config` - Loaded configuration containing target topology
///
/// # Returns
/// `Ok(true)` if a migration was performed, `Ok(false)` if no migration was necessary.
///
/// # Example
/// ```no_run
/// use crane::config::Config;
/// use crane::postgres_unit::migration::maybe_migrate_postgres_topology;
///
/// # async fn deploy(cfg: &Config) -> anyhow::Result<()> {
/// if maybe_migrate_postgres_topology(cfg).await? {
///     println!("Topology migration completed");
/// }
/// # Ok(())
/// # }
/// ```
pub async fn maybe_migrate_postgres_topology(config: &config::Config) -> anyhow::Result<bool> {
    let pg_nodes = config_get_nodes(config, "postgres");
    if pg_nodes.is_empty() {
        return Ok(false);
    }

    let primary_node = match pg_nodes.first() {
        Some(n) => n,
        None => return Ok(false),
    };

    let interactor = get_server_interactor(&primary_node.name)?;
    let pg_version = get_pg_version(config);

    let current_mode = detect_node_postgres_mode(&*interactor, &pg_version);
    let target_mode = if postgres_is_minimal(config) {
        PostgresDeploymentMode::Minimal
    } else {
        PostgresDeploymentMode::Cluster
    };

    if current_mode == PostgresDeploymentMode::Uninstalled || current_mode == target_mode {
        return Ok(false);
    }

    match (current_mode, target_mode) {
        (PostgresDeploymentMode::Minimal, PostgresDeploymentMode::Cluster) => {
            migrate_minimal_to_cluster(config).await?;
            Ok(true)
        }
        (PostgresDeploymentMode::Cluster, PostgresDeploymentMode::Minimal) => {
            migrate_cluster_to_minimal(config).await?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// Migrates a standalone single-node PostgreSQL instance to a high-availability Patroni+etcd cluster.
async fn migrate_minimal_to_cluster(config: &config::Config) -> anyhow::Result<()> {
    println!("\n========================================================");
    println!("  MIGRATION DETECTED: Minimal Standalone -> HA Cluster");
    println!("========================================================\n");

    let pg_version = get_pg_version(config);
    let pg_nodes = config_get_nodes(config, "postgres");
    let primary_node = pg_nodes
        .first()
        .ok_or_else(|| anyhow::anyhow!("No postgres node configured for migration"))?;
    let interactor = get_server_interactor(&primary_node.name)?;

    // 1. Verify health of source database
    println!(
        "Step 1/6: Verifying source PostgreSQL health on {}...",
        primary_node.name
    );
    let health_check = interactor.psql(Some("SELECT 1;"), None, None, true)?;
    if health_check.stdout.trim() != "1" {
        anyhow::bail!(
            "Cannot migrate: Source PostgreSQL on {} is not responsive",
            primary_node.name
        );
    }

    // 2. Export full data dump to temporary file
    let dump_file = "/tmp/crane_migration_minimal_to_cluster.sql";
    println!("Step 2/6: Exporting full data dump to {}...", dump_file);
    let dump_cmd = format!(
        "sudo -u postgres pg_dumpall --clean --if-exists > {}",
        dump_file
    );
    let dump_res = interactor.cmd(&dump_cmd)?;
    if dump_res.exit_code != 0 {
        anyhow::bail!("Failed to export database dump: {}", dump_res.stderr);
    }

    // Retain safety copy in backups directory
    let safety_copy = format!(
        "{}/migration_safety_backup_{}.sql",
        interactor.server_paths().pg_backup_dir,
        chrono::Utc::now().timestamp()
    );
    let _ = interactor.mkdir(&interactor.server_paths().pg_backup_dir);
    let _ = interactor.cmd(&format!("sudo cp {} {}", dump_file, safety_copy));

    // 3. Stop standalone PostgreSQL
    println!("Step 3/6: Stopping standalone PostgreSQL service...");
    let _ = interactor.stop_service("postgresql");
    let _ = interactor.disable_service("postgresql");
    let _ = interactor.kill_postgres_processes();

    // 4. Archive old data directory
    println!("Step 4/6: Archiving standalone data directory...");
    backup_pg_dir(&pg_version, &*interactor)?;

    // 5. Deploy Patroni + etcd cluster
    println!("Step 5/6: Deploying Patroni + etcd cluster across all nodes...");
    crate::etcd_unit::etcd_clear_dcs_state(&*interactor);
    setup_postgres_cluster(config).await?;

    // 6. Restore data dump on newly elected Patroni leader
    println!("Step 6/6: Importing database dump into Patroni leader...");
    let leader = pg_get_primary(config)?
        .ok_or_else(|| anyhow::anyhow!("No leader elected after cluster setup"))?;
    let leader_interactor = get_server_interactor(&leader.name)?;

    let restore_cmd = format!("sudo -u postgres psql -f {}", dump_file);
    let restore_res = leader_interactor.cmd(&restore_cmd)?;
    if restore_res.exit_code != 0 {
        println!("Notice: data import stderr: {}", restore_res.stderr);
    }

    let _ = leader_interactor.rm(dump_file);
    println!("\nMigration Minimal -> Cluster completed successfully!\n");
    Ok(())
}

/// Migrates a high-availability Patroni+etcd cluster to a standalone single-node PostgreSQL instance.
async fn migrate_cluster_to_minimal(config: &config::Config) -> anyhow::Result<()> {
    println!("\n========================================================");
    println!("  MIGRATION DETECTED: HA Cluster -> Minimal Standalone");
    println!("========================================================\n");

    let pg_nodes = config_get_nodes(config, "postgres");
    let minimal_node = pg_nodes
        .first()
        .ok_or_else(|| anyhow::anyhow!("No postgres node configured for minimal mode"))?;
    let minimal_interactor = get_server_interactor(&minimal_node.name)?;

    // 1. Locate current cluster leader
    println!("Step 1/5: Locating active cluster leader...");
    let current_leader = match pg_get_primary(config) {
        Ok(Some(leader)) => leader,
        _ => minimal_node.clone(),
    };
    let leader_interactor = get_server_interactor(&current_leader.name)?;

    // 2. Export full data dump from leader
    let dump_file = "/tmp/crane_migration_cluster_to_minimal.sql";
    println!(
        "Step 2/5: Exporting full data dump from leader {}...",
        current_leader.name
    );
    let dump_cmd = format!(
        "sudo -u postgres pg_dumpall --clean --if-exists > {}",
        dump_file
    );
    let dump_res = leader_interactor.cmd(&dump_cmd)?;
    if dump_res.exit_code != 0 {
        anyhow::bail!("Failed to export database dump: {}", dump_res.stderr);
    }

    // Transfer dump file to minimal node if leader is on a different node
    if current_leader.internal_ip != minimal_node.internal_ip {
        let dump_content = leader_interactor.read_file(dump_file)?;
        minimal_interactor.create_file(dump_file, &dump_content)?;
        let _ = leader_interactor.rm(dump_file);
    }

    // Retain safety copy
    let safety_copy = format!(
        "{}/migration_safety_backup_{}.sql",
        minimal_interactor.server_paths().pg_backup_dir,
        chrono::Utc::now().timestamp()
    );
    let _ = minimal_interactor.mkdir(&minimal_interactor.server_paths().pg_backup_dir);
    let _ = minimal_interactor.cmd(&format!("sudo cp {} {}", dump_file, safety_copy));

    // 3. Stop and disable Patroni and etcd across all nodes
    println!("Step 3/5: Decommissioning Patroni and etcd on all nodes...");
    for node in &pg_nodes {
        if let Ok(node_interactor) = get_server_interactor(&node.name) {
            let _ = node_interactor.stop_service("patroni");
            let _ = node_interactor.disable_service("patroni");
            let _ = node_interactor.stop_service("etcd");
            let _ = node_interactor.disable_service("etcd");
            let _ = node_interactor.kill_postgres_processes();
            let _ = node_interactor.rm(&node_interactor.server_paths().patroni_config_path);
        }
    }

    // 4. Deploy standalone PostgreSQL on the target minimal node
    println!(
        "Step 4/5: Deploying standalone PostgreSQL on node {}...",
        minimal_node.name
    );
    setup_postgres_minimal(config).await?;

    // 5. Restore data dump into standalone PostgreSQL
    println!("Step 5/5: Importing database dump into standalone PostgreSQL...");
    let restore_cmd = format!("sudo -u postgres psql -f {}", dump_file);
    let restore_res = minimal_interactor.cmd(&restore_cmd)?;
    if restore_res.exit_code != 0 {
        println!("Notice: data import stderr: {}", restore_res.stderr);
    }

    let _ = minimal_interactor.rm(dump_file);
    println!("\nMigration Cluster -> Minimal completed successfully!\n");
    Ok(())
}
