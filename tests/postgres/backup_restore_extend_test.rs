// RUN:
// RUST_BACKTRACE=1 cargo nextest run test_backup_restore_extend -- --no-capture

use std::sync::Arc;

use crane::{config::Config, postgres_unit::entity::BackupMetadata};

#[tokio::test]
async fn test_backup_restore_extend() {
    let config_path = std::path::Path::new("tests/postgres/crane.toml");
    let config = read_config_toml_file(config_path).expect("Failed to load crane.toml");

    //Deploy
    // crane::commands::deploy::run_deploy_command(&config, config_path, true)
    //     .await
    //     .expect("deploy failed");

    // Retrieve leader node and connect
    let primary_node = postgres_get_primary(&config)
        .expect("Failed to get leader node")
        .expect("No active PostgreSQL leader found");

    let interactor = crane::postgres_unit::helper::connect_to_node(&primary_node, &config)
        .expect("Failed to connect to primary node");

    println!("Step 1: DROP and CREATE test_table");
    run_sql(
        &*interactor,
        "DROP TABLE IF EXISTS test_table; CREATE TABLE test_table (id INT);",
    );

    println!("Step 2: insert 1 to test_table");
    run_sql(&*interactor, "INSERT INTO test_table VALUES (1);");

    // ============================================================
    // FULL BACKUP #1
    // ============================================================
    let _backup_meta_full_1 = testw_full_backup(&config, "full1").unwrap();

    // ============================================================
    // INSERT 2
    // ============================================================
    let pitr_insert_2 = testw_insert(interactor.clone(), 2).unwrap();

    // ============================================================
    // CREATE INCREMENTAL BACKUP #1
    // ============================================================
    let backup_incr_insert_2 = testw_incr_backup(&config, "incr_insert_2").unwrap();

    // ============================================================
    // RESTORE FROM INCREMENTAL BACKUP #1
    // ============================================================
    testw_restore(&config, &backup_incr_insert_2, Some(&pitr_insert_2))
        .await
        .unwrap();

    // ============================================================
    // ASSERT DATA FROM INCREMENTAL BACKUP TEST #1
    // ============================================================
    let interactor = crane::postgres_unit::helper::connect_to_node(&primary_node, &config).unwrap();

    testw_assert_table(interactor, "1\n2").unwrap();
}

fn testw_full_backup(config: &Config, backup_name: &str) -> anyhow::Result<BackupMetadata> {
    println!("Create full backup");

    let backup_meta =
        backup_from_config_wrapper(&config, "full", Some(backup_name)).expect("Full backup failed");
    assert_eq!(backup_meta.backup_type, "FULL");
    assert_eq!(backup_meta.label, Some(backup_name.to_string()));

    Ok(backup_meta)
}

fn testw_incr_backup(config: &Config, backup_name: &str) -> anyhow::Result<BackupMetadata> {
    println!("Create incremental backup");

    let backup_meta = backup_from_config_wrapper(&config, "incr", Some(backup_name))
        .expect("Incremental backup failed");
    assert_eq!(backup_meta.backup_type, "INCR");

    Ok(backup_meta)
}

async fn testw_restore(
    config: &Config,
    backup_meta: &BackupMetadata,
    pitr: Option<&str>,
) -> anyhow::Result<()> {
    println!(
        "\nRestore from backup id {} with pitr: {:?}\n",
        backup_meta.id, pitr
    );

    run_postgres_restore_cmd(&config, &backup_meta.id, None, pitr)
        .await
        .expect("Restore from backup failed");

    Ok(())
}

fn testw_insert(interactor: Arc<dyn ServerInteractor>, value: i32) -> anyhow::Result<String> {
    println!("Step 9: insert {} to table", value);
    let _insert = run_sql(
        &*interactor,
        &format!("INSERT INTO test_table VALUES ({});", value),
    );
    std::thread::sleep(std::time::Duration::from_secs(1));
    let pitr_insert_time = run_sql(
        &*interactor,
        "SELECT to_char(clock_timestamp(), 'YYYY-MM-DD HH24:MI:SS.MS');",
    );
    let pitr_insert_time = pitr_insert_time
        .lines()
        .map(|s| s.trim())
        .find(|s| s.len() >= 19)
        .unwrap_or("")
        .to_string();
    println!("\nSTORE pitr_insert_time {}: {}\n", value, pitr_insert_time);
    Ok(pitr_insert_time)
}

fn testw_assert_table(interactor: Arc<dyn ServerInteractor>, expected: &str) -> anyhow::Result<()> {
    println!("Assert test_table content: expected {} rows", expected);
    let rows = run_sql(&*interactor, "SELECT id FROM test_table ORDER BY id;");
    // dbg!(&rows);
    assert_eq!(rows, expected, "EXPECT {} BUT GOT {}", expected, rows);
    Ok(())
}
