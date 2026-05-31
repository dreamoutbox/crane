use crate::{
    postgres_unit::{
        entity::BackupRegistry,
        helper::{connect_to_node, get_backups_from_s3, get_pg_version, postgres_get_leader},
    },
    s3::{get_s3_config, s3_client::RealS3Client},
};

pub fn run_restore_cmd(
    config: &crate::config::Config,
    target_backup_id: &str,  // <id>: targetb backup ID
    base_id: Option<&str>,   // --base: stop chain walk here (inclusive)
    pitr_time: Option<&str>, // --pitr "YYYY-MM-DD HH:MM:SS" UTC
) -> anyhow::Result<()> {
    println!(
        "\nStart restore: target_backup_id={} base_id={} pitr_time={}\n",
        target_backup_id,
        base_id.as_deref().unwrap_or("NONE"),
        pitr_time.as_deref().unwrap_or("NONE")
    );

    let s3_config = get_s3_config(&config)?;
    let primary_node = match postgres_get_leader(&config)? {
        Some(node) => node,
        None => config
            .nodes
            .iter()
            .find(|n| n.roles.contains(&"postgres".to_string()))
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("No PostgreSQL nodes found in configuration"))?,
    };

    let pg_version = get_pg_version(&config);
    let s3_client = RealS3Client::new(&s3_config)?;

    let backups = get_backups_from_s3(&s3_client)?;
    let registry = BackupRegistry { backups };

    let target_backup = registry
        .backups
        .iter()
        .find(|b| b.id == target_backup_id)
        .ok_or_else(|| anyhow::anyhow!("Backup ID '{}' not found in registry", target_backup_id))?;

    // Validate --base exists in registry if specified
    if let Some(forced_base) = base_id {
        if !registry.backups.iter().any(|b| b.id == forced_base) {
            anyhow::bail!("Base backup ID '{}' not found in registry", forced_base);
        }
    }

    // Build restore chain, stopping at base_id when specified
    let mut chain = Vec::new();
    let mut current = target_backup.clone();
    chain.push(current.clone());

    while let Some(ref next_base_id) = current.base {
        // If the user specified --base, stop once we've included that backup
        if let Some(forced_base) = base_id {
            if current.id == forced_base {
                break;
            }
        }
        let parent = registry
            .backups
            .iter()
            .find(|b| &b.id == next_base_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Broken backup chain: parent backup ID '{}' not found in registry",
                    next_base_id
                )
            })?;
        chain.push(parent.clone());
        current = parent.clone();
    }

    chain.reverse();

    println!("Backup chain to restore:");
    for item in &chain {
        println!(" - ID: {} ({})", item.id, item.backup_type);
    }

    // Validate --pitr is after the oldest backup in the chain (chain[0] after reverse)
    if let Some(pitr) = pitr_time {
        let pitr_dt =
            chrono::NaiveDateTime::parse_from_str(pitr, "%Y-%m-%d %H:%M:%S").map_err(|_| {
                anyhow::anyhow!(
                    "--pitr must be in 'YYYY-MM-DD HH:MM:SS' format. got `{}`",
                    pitr
                )
            })?;

        let base_backup = &chain[0];
        if let Some(ref taken_at) = base_backup.taken_at {
            let base_dt = chrono::NaiveDateTime::parse_from_str(taken_at, "%Y-%m-%d %H:%M:%S")
                .map_err(|_| anyhow::anyhow!("Base backup has invalid taken_at: '{}'", taken_at))?;

            if pitr_dt <= base_dt {
                anyhow::bail!(
                    "--pitr time '{}' must be after the base backup time '{}' (backup ID: {})",
                    pitr,
                    taken_at,
                    base_backup.id
                );
            }
        }
    }

    let interactor = connect_to_node(&primary_node, &config)?;

    println!("Restoring database to backup ID: {}...", target_backup_id);
    if let Some(t) = pitr_time {
        println!("Point-in-time recovery target: {}", t);
    }

    crate::postgres_unit::restore::postgres_restore(
        config,
        &primary_node,
        &*interactor,
        &s3_client,
        &pg_version,
        target_backup,
        &chain,
        pitr_time,
    )?;

    println!("Restore complete!\n");

    Ok(())
}
