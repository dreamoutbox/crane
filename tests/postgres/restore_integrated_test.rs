// RUN:
// RUST_BACKTRACE=1 cargo nextest run test_restore_integrated_workflow -- --no-capture

use crane::{
    commands::{postgres_backup::backup_from_config_wrapper, postgres_restore::run_restore_cmd},
    config::read_config_toml_file,
    postgres_unit::helper::postgres_get_leader,
};

#[test]
fn test_restore_integrated_workflow() {
    let config_path = std::path::Path::new("tests/postgres/crane.toml");
    let config = read_config_toml_file(config_path).expect("Failed to load crane.toml");

    //Deploy
    crane::commands::deploy::run(&config, config_path, true).expect("deploy failed");

    // Retrieve leader node and connect
    let primary_node = postgres_get_leader(&config)
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

    println!("Step 3: insert 2 to test_table");
    run_sql(&*interactor, "INSERT INTO test_table VALUES (2);");

    // ============================================================
    // FULL BACKUP #1
    // ============================================================
    println!("Step 4: create full backup");
    let full_backup =
        backup_from_config_wrapper(&config, config_path, "full").expect("Full backup failed");
    assert_eq!(full_backup.backup_type, "FULL");

    // ============================================================
    // SIMULATE DATA LOSS
    // ============================================================
    println!("Step 5: DROP test_table (SIMULATE DATA LOSS)");
    run_sql(&*interactor, "DROP TABLE test_table;");
    println!("Step 6: assert table is DROP");
    let exists = run_sql(
        &*interactor,
        "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = 'test_table');",
    );
    assert_eq!(exists, "f");

    // ============================================================
    // RESTORE FROM FULL BACKUP
    // ============================================================
    println!("Step 7: restore from full backup");
    run_restore_cmd(&config, &full_backup.id, None, None).expect("Restore from full backup failed");

    // ============================================================
    //  ASSERT DATA FROM FULL BACKUP
    // ============================================================
    // Reconnect after restore (since postgres was restarted)
    let interactor = crane::postgres_unit::helper::connect_to_node(&primary_node, &config)
        .expect("Failed to reconnect to primary node");

    println!("Step 8: assert table have 1,2 in table");
    let rows = run_sql(&*interactor, "SELECT id FROM test_table ORDER BY id;");
    assert_eq!(rows, "1\n2");

    // ============================================================
    // PREPARE DATA FOR PITR RESTORE #1
    // ============================================================
    println!("Step 9: insert 3 to table");
    let pitr_time_insert_3 = run_sql(
        &*interactor,
        "INSERT INTO test_table VALUES (3) RETURNING to_char(clock_timestamp() + interval '1 second' , 'YYYY-MM-DD HH24:MI:SS');", //TEMP:clock_timestamp() + interval '1 second'
    );
    let pitr_time_insert_3 = pitr_time_insert_3
        .lines()
        .map(|s| s.trim())
        .find(|s| s.len() == 19 && s.chars().nth(4) == Some('-') && s.chars().nth(13) == Some(':'))
        .unwrap_or("")
        .to_string();
    println!("STORE pitr_time_insert_3: {}", pitr_time_insert_3);

    // ============================================================
    // CREATE INCREMENTAL BACKUP #1 (will auto-fallback to FULL after timeline change)
    // ============================================================
    println!("Step 10: create incremental backup #1 (expects FULL fallback after restore)");
    std::thread::sleep(std::time::Duration::from_secs(2));
    let incr_backup_1 = backup_from_config_wrapper(&config, config_path, "incr")
        .expect("Incremental backup #1 failed");
    // After restore, timeline changes so incremental auto-falls back to FULL
    assert_eq!(incr_backup_1.backup_type, "FULL");

    // ============================================================
    // PREPARE DATA FOR PITR RESTORE #2
    // ============================================================
    println!("Step 11: insert 4 to table");
    let pitr_time_insert_4 = run_sql(
        &*interactor,
        "INSERT INTO test_table VALUES (4) RETURNING to_char(clock_timestamp(), 'YYYY-MM-DD HH24:MI:SS');",
    );
    let pitr_time_insert_4 = pitr_time_insert_4
        .lines()
        .map(|s| s.trim())
        .find(|s| s.len() == 19 && s.chars().nth(4) == Some('-') && s.chars().nth(13) == Some(':'))
        .unwrap_or("")
        .to_string();
    println!("STORE pitr_time_insert_4: {}", pitr_time_insert_4);

    // ============================================================
    // SIMULATE DATA LOSS
    // ============================================================
    std::thread::sleep(std::time::Duration::from_secs(1));
    let pitr_time_before_drop = run_sql(
        &*interactor,
        "SELECT to_char(clock_timestamp(), 'YYYY-MM-DD HH24:MI:SS');",
    );
    let pitr_time_before_drop = pitr_time_before_drop
        .lines()
        .map(|s| s.trim())
        .find(|s| s.len() == 19 && s.chars().nth(4) == Some('-') && s.chars().nth(13) == Some(':'))
        .unwrap_or("")
        .to_string();
    println!("STORE pitr_time_before_drop: {}", pitr_time_before_drop);

    std::thread::sleep(std::time::Duration::from_secs(1));
    println!("Step 11.5: DROP test_table (SIMULATE DATA LOSS)");
    run_sql(&*interactor, "DROP TABLE test_table;");

    // ============================================================
    // CREATE INCREMENTAL BACKUP #2 (same timeline as backup #1, should be real INCR)
    // ============================================================
    println!("Step 12: create incremental backup #2");
    std::thread::sleep(std::time::Duration::from_secs(2));
    let incr_backup_2 = backup_from_config_wrapper(&config, config_path, "incr")
        .expect("Incremental backup #2 failed");
    assert_eq!(incr_backup_2.backup_type, "INCR");

    // ============================================================
    // RESTORE FROM INCREMENTAL BACKUP #1
    // ============================================================
    let pitr_arg = if incr_backup_1.backup_type == "FULL" {
        None
    } else {
        Some(pitr_time_insert_3.as_str())
    };
    println!(
        "Step 15: restore from backup #1 (type: {}) with pitr: {:?}",
        incr_backup_1.backup_type, pitr_arg
    );
    run_restore_cmd(&config, &incr_backup_1.id, None, pitr_arg)
        .expect("Restore from backup #1 failed");

    // ============================================================
    // ASSERT DATA FROM INCREMENTAL BACKUP TEST #1
    // ============================================================
    // Reconnect after restore
    let interactor = crane::postgres_unit::helper::connect_to_node(&primary_node, &config)
        .expect("Failed to reconnect to primary node");

    println!("Step 16: assert table have 1,2,3 in table");
    let rows = run_sql(&*interactor, "SELECT id FROM test_table ORDER BY id;");
    assert_eq!(rows, "1\n2\n3");

    // ============================================================
    // SIMULATE DATA LOSS
    // ============================================================
    println!("Step 17: DROP test_table (SIMULATE DATA LOSS)");
    run_sql(&*interactor, "DROP TABLE test_table;");
    println!("Step 18: assert table is DROP");
    let exists = run_sql(
        &*interactor,
        "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = 'test_table');",
    );
    assert_eq!(exists, "f");

    // ============================================================
    // RESTORE FROM INCREMENTAL BACKUP #2
    // ============================================================
    println!(
        "Step 19: restore from incremental backup #2 with --pitr {}",
        pitr_time_before_drop
    );
    run_restore_cmd(
        &config,
        &incr_backup_2.id,
        None,
        Some(&pitr_time_before_drop),
    )
    .expect("PITR restore to SQL `INSERT 5` failed");

    // ============================================================
    // ASSERT DATA FROM INCREMENTAL BACKUP TEST #2
    // ============================================================
    // Reconnect after restore
    let interactor = crane::postgres_unit::helper::connect_to_node(&primary_node, &config)
        .expect("Failed to reconnect to primary node");

    println!("Step 20: assert table have 1,2,3,4 in table");
    let rows = run_sql(&*interactor, "SELECT id FROM test_table ORDER BY id;");
    assert_eq!(rows, "1\n2\n3\n4");
}

pub fn run_sql(interactor: &dyn ServerInteractor, sql: &str) -> String {
    let cmd = format!("sudo -u postgres psql -d mydb -t -A -c {:?}", sql);
    let out = interactor.cmd(&cmd).expect("SQL execution failed");
    assert_eq!(out.exit_code, 0, "SQL failed: {}", out.stderr);
    out.stdout.trim().to_string()
}
