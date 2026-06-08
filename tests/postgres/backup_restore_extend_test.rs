// RUN:
// RUST_BACKTRACE=1 cargo nextest run test_backup_restore_extend -- --no-capture

use crate::common_helper::reset_docker_compose;
use crate::helper::run_sql;
use crane::commands::postgres_backup::backup_from_config_wrapper;
use crane::config::{Config, read_config_toml_file};
use crane::postgres_unit::entity::BackupMetadata;
use crane::postgres_unit::helper::pg_get_primary;
use crane::postgres_unit::restore::postgres_restore;
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
    let primary_node = pg_get_primary(&config)
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
    let _pitr_insert_1 = tw_insert(interactor.clone(), 1).unwrap();

    // ============================================================
    // FULL BACKUP #1
    // ============================================================
    let _backup_meta_full_1 = tw_full_backup(&config, "full1").unwrap();

    // ============================================================
    // INSERT 2
    // ============================================================
    let pitr_insert_2 = tw_insert(interactor.clone(), 2).unwrap();

    // ============================================================
    // SIMULATE DATA LOSS
    // ============================================================
    println!("\n\nSIMULATE DATA LOSS\n\n");
    let _time_before_drop = tw_drop(interactor.clone()).await;

    // ============================================================
    // CREATE INCREMENTAL BACKUP #1
    // ============================================================
    let backup_incr1 = tw_incr_backup(&config, "incr1").unwrap();

    // ============================================================
    // RESTORE FROM INCREMENTAL BACKUP #1
    // ============================================================
    tw_restore(&config, &backup_incr1, Some(&pitr_insert_2))
        .await
        .unwrap();

    // ============================================================
    // ASSERT DATA FROM INCREMENTAL BACKUP TEST #1
    // ============================================================
    let interactor = get_server_interactor(&primary_node.name).unwrap();

    tw_assert_table(interactor, "1\n2").unwrap();

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
    let _backup_full_2 = tw_full_backup(&config, "full2").unwrap();

    // Reconnect to get a fresh interactor
    let interactor = get_server_interactor(&primary_node.name).unwrap();

    // ============================================================
    // INSERT 3 TO TABLE
    // ============================================================
    let _pitr_insert_3 = tw_insert(interactor.clone(), 3).unwrap();

    // ============================================================
    // DROP TABLE (SIMULATE DATA LOSS)
    // ============================================================
    println!("\n\nSIMULATE DATA LOSS 2\n\n");
    let time_before_drop = tw_drop(interactor.clone()).await;

    // ============================================================
    // TAKE INCREMENTAL BACKUP
    // ============================================================
    let backup_incr2 = tw_incr_backup(&config, "incr2").unwrap();

    // ============================================================
    // RESTORE FROM INCREMENTAL BACKUP
    // ============================================================
    tw_restore(&config, &backup_incr2, Some(&time_before_drop))
        .await
        .unwrap();

    // ============================================================
    // ASSERT 1,2,3 IN TABLE
    // ============================================================
    let interactor = get_server_interactor(&primary_node.name).unwrap();
    tw_assert_table(interactor, "1\n2\n3").unwrap();
}

fn tw_full_backup(config: &Config, backup_name: &str) -> anyhow::Result<BackupMetadata> {
    println!("Create full backup");

    let backup_meta =
        backup_from_config_wrapper(&config, "full", Some(backup_name)).expect("Full backup failed");
    assert_eq!(backup_meta.backup_type, "FULL");
    assert_eq!(backup_meta.label, Some(backup_name.to_string()));

    Ok(backup_meta)
}

fn tw_incr_backup(config: &Config, backup_name: &str) -> anyhow::Result<BackupMetadata> {
    println!("Create incremental backup");

    let backup_meta = backup_from_config_wrapper(&config, "incr", Some(backup_name))
        .expect("Incremental backup failed");
    assert_eq!(backup_meta.backup_type, "INCR");

    Ok(backup_meta)
}

async fn tw_restore(
    config: &Config,
    backup_meta: &BackupMetadata,
    pitr: Option<&str>,
) -> anyhow::Result<()> {
    println!(
        "\nRestore from backup id {} with pitr: {:?}\n",
        backup_meta.id, pitr
    );

    postgres_restore(&config, &backup_meta.id, None, pitr)
        .await
        .expect("Restore from backup failed");

    Ok(())
}

fn get_time_for_pitr() -> String {
    let pitr_time = chrono::Utc::now()
        .format("%Y-%m-%d %H:%M:%S%.6f")
        .to_string();

    pitr_time
}

fn tw_insert(interactor: Arc<dyn ServerInteractor>, value: i32) -> anyhow::Result<String> {
    println!("Insert {} to table", value);
    let _pitr_insert_time = run_sql(
        &*interactor,
        &format!("INSERT INTO test_table VALUES ({});", value),
    );

    let insert_time = get_time_for_pitr();

    println!("\ntw_insert {} insert_time={:?}\n", value, insert_time);

    Ok(insert_time)
}

fn tw_assert_table(interactor: Arc<dyn ServerInteractor>, expected: &str) -> anyhow::Result<()> {
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

async fn tw_drop(interactor: Arc<dyn ServerInteractor>) -> String {
    let time_before_drop = get_time_for_pitr();

    println!(
        "\nDROP table test_table. time_before_drop: {}\n",
        time_before_drop
    );

    run_sql(&*interactor, "DROP TABLE test_table;");

    time_before_drop
}
