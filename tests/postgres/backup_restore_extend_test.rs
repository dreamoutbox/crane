// RUN:
// RUST_BACKTRACE=1 cargo nextest run test_backup_restore_extend -- --no-capture

use crate::helper::{reset_docker_compose, run_sql};
use crane::commands::postgres_backup::backup_from_config_wrapper;
use crane::commands::postgres_restore::run_postgres_restore_cmd;
use crane::config::{Config, read_config_toml_file};
use crane::postgres_unit::entity::BackupMetadata;
use crane::postgres_unit::helper::postgres_get_primary;
use crane::server_interactor::get_server_interactor;
use crane::server_interactor::server_interactor_trait::ServerInteractor;
use std::sync::Arc;

#[tokio::test]
async fn test_backup_restore_extend() {
    println!("Recreating Docker compose...");
    reset_docker_compose().await;

    let config_path = std::path::Path::new("tests/postgres/crane.toml");
    let config = read_config_toml_file(config_path).expect("Failed to load crane.toml");

    //Deploy
    crane::commands::deploy::run_deploy_command(&config, config_path, true)
        .await
        .expect("deploy failed");

    // Retrieve leader node and connect
    let primary_node = postgres_get_primary(&config)
        .expect("Failed to get leader node")
        .expect("No active PostgreSQL leader found");

    let interactor =
        get_server_interactor(&primary_node.name).expect("Failed to connect to primary node");

    println!("Step 1: DROP and CREATE test_table");
    run_sql(
        &*interactor,
        "DROP TABLE IF EXISTS test_table; CREATE TABLE test_table (id INT);",
    );

    println!("Step 2: insert 1 to test_table");
    // run_sql(&*interactor, "INSERT INTO test_table VALUES (1);");
    let _pitr_insert_1 = testw_insert(interactor.clone(), 1).unwrap();

    // ============================================================
    // FULL BACKUP #1
    // ============================================================
    let _backup_meta_full_1 = testw_full_backup(&config, "full1").unwrap();

    // ============================================================
    // INSERT 2
    // ============================================================
    let pitr_insert_2 = testw_insert(interactor.clone(), 2).unwrap();

    // ============================================================
    // SIMULATE DATA LOSS
    // ============================================================
    println!("\n\nSIMULATE DATA LOSS\n\n");
    run_sql(&*interactor, "DROP TABLE test_table;");

    // ============================================================
    // CREATE INCREMENTAL BACKUP #1
    // ============================================================
    let backup_incr1 = testw_incr_backup(&config, "incr1").unwrap();

    // ============================================================
    // RESTORE FROM INCREMENTAL BACKUP #1
    // ============================================================
    testw_restore(&config, &backup_incr1, Some(&pitr_insert_2))
        .await
        .unwrap();

    // ============================================================
    // ASSERT DATA FROM INCREMENTAL BACKUP TEST #1
    // ============================================================
    let interactor = get_server_interactor(&primary_node.name).unwrap();

    testw_assert_table(interactor, "1\n2").unwrap();

    // ============================================================
    // EXPECT ERROR WHEN RUNNING INCREMENTAL TEST
    // BECAUSE WE AREN'T HAVE BASE FULL BACKUP AFTER RESTORE WITH INCREMENTAL BACKUP
    // ============================================================

    let incr_backup_result = backup_from_config_wrapper(&config, "incr", None);
    dbg!(&incr_backup_result);
    assert!(incr_backup_result.is_err());
    // expect error contains `consider full backup first!`
    assert!(
        incr_backup_result
            .err()
            .unwrap()
            .to_string()
            .contains("consider full backup first!")
    );

    // ============================================================
    // TAKE FULL BACKUP
    // ============================================================
    let _backup_full_2 = testw_full_backup(&config, "full2").unwrap();

    // Reconnect to get a fresh interactor
    let interactor = get_server_interactor(&primary_node.name).unwrap();

    // ============================================================
    // INSERT 3 TO TABLE
    // ============================================================
    let pitr_insert_3 = testw_insert(interactor.clone(), 3).unwrap();

    // ============================================================
    // DROP TABLE (SIMULATE DATA LOSS)
    // ============================================================
    println!("\n\nSIMULATE DATA LOSS 2\n\n");
    run_sql(&*interactor, "DROP TABLE test_table;");

    // ============================================================
    // TAKE INCREMENTAL BACKUP
    // ============================================================
    let backup_incr2 = testw_incr_backup(&config, "incr2").unwrap();

    // ============================================================
    // RESTORE FROM INCREMENTAL BACKUP
    // ============================================================
    testw_restore(&config, &backup_incr2, Some(&pitr_insert_3))
        .await
        .unwrap();

    // ============================================================
    // ASSERT 1,2,3 IN TABLE
    // ============================================================
    let interactor = get_server_interactor(&primary_node.name).unwrap();
    testw_assert_table(interactor, "1\n2\n3").unwrap();
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
    println!("Insert {} to table", value);
    let _pitr_insert_time = run_sql(
        &*interactor,
        &format!("INSERT INTO test_table VALUES ({});", value),
    );

    let pitr_insert_time = chrono::Utc::now()
        .format("%Y-%m-%d %H:%M:%S%.6f")
        .to_string();

    println!(
        "\nSTORE pitr_insert_time {}: {:?}\n",
        value, pitr_insert_time
    );

    Ok(pitr_insert_time)
}

fn testw_assert_table(interactor: Arc<dyn ServerInteractor>, expected: &str) -> anyhow::Result<()> {
    println!("\nAssert test_table: expected {}\n", expected);
    let rows = run_sql(&*interactor, "SELECT id FROM test_table ORDER BY id;");
    // dbg!(&rows);
    assert_eq!(
        rows, expected,
        "SELECT test_table EXPECT `{}`. BUT GOT `{}`.",
        expected, rows
    );
    Ok(())
}
