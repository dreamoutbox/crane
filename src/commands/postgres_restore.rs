pub async fn run_postgres_restore_cmd(
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

    crate::postgres_unit::restore::postgres_restore(config, target_backup_id, base_id, pitr_time)
        .await?;

    println!("Restore complete!\n");

    Ok(())
}
