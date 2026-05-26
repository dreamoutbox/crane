// RUN:
// cargo nextest run --test postgres -- test_restore_integrated_workflow --no-capture

#[test]
fn test_restore_integrated_workflow() {
    let config_path = std::path::Path::new("demo/crane.toml");

    // Load config
    let config = crane::config::load_config(config_path).expect("Failed to load crane.toml");

    // Retrieve leader node and connect
    let primary_node = crane::postgres_unit::tasks::postgres_get_leader(
        &config,
        crane::server_interactor::get_interactor,
    )
    .expect("Failed to get leader node")
    .expect("No active PostgreSQL leader found");

    let interactor = crane::postgres_unit::helper::connect_to_node(
        &primary_node,
        &config,
        crane::server_interactor::get_interactor,
    )
    .expect("Failed to connect to primary node");

    let run_sql = |interactor: &dyn ServerInteractor, sql: &str| -> String {
        let cmd = format!("sudo -u postgres psql -d myapp -t -A -c {:?}", sql);
        let out = interactor.cmd(&cmd).expect("SQL execution failed");
        assert_eq!(out.exit_code, 0, "SQL failed: {}", out.stderr);
        out.stdout.trim().to_string()
    };

    // --- FULL BACKUP TEST #1 ---
    println!("Step 1: delete and create integrated_test_table");
    run_sql(
        &*interactor,
        "DROP TABLE IF EXISTS integrated_test_table; CREATE TABLE integrated_test_table (id INT);",
    );

    println!("Step 2: insert 1 to integrated_test_table");
    run_sql(
        &*interactor,
        "INSERT INTO integrated_test_table VALUES (1);",
    );

    println!("Step 3: insert 2 to integrated_test_table");
    run_sql(
        &*interactor,
        "INSERT INTO integrated_test_table VALUES (2);",
    );

    println!("Step 4: create full backup");
    let full_backup = crane::commands::postgres::run_backup_cmd(
        config_path,
        "full",
        crane::server_interactor::get_interactor,
    )
    .expect("Full backup failed");
    assert_eq!(full_backup.backup_type, "FULL");

    println!("Step 5: delete integrated_test_table");
    run_sql(&*interactor, "DROP TABLE integrated_test_table;");

    println!("Step 6: assert table is deleted");
    let exists = run_sql(
        &*interactor,
        "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = 'integrated_test_table');",
    );
    assert_eq!(exists, "f");

    println!("Step 7: restore from full backup");
    crane::commands::postgres::run_restore_cmd(
        config_path,
        &full_backup.id,
        None,
        None,
        crane::server_interactor::get_interactor,
    )
    .expect("Restore from full backup failed");

    // Reconnect after restore (since postgres was restarted)
    let interactor = crane::postgres_unit::helper::connect_to_node(
        &primary_node,
        &config,
        crane::server_interactor::get_interactor,
    )
    .expect("Failed to reconnect to primary node");

    println!("Step 8: assert table have 1,2 in table");
    let rows = run_sql(
        &*interactor,
        "SELECT id FROM integrated_test_table ORDER BY id;",
    );
    assert_eq!(rows, "1\n2");

    // --- INCREMENTAL BACKUP TEST ---
    println!("Step 9: insert 3 to table");
    run_sql(
        &*interactor,
        "INSERT INTO integrated_test_table VALUES (3);",
    );

    // Store time after inserting 3
    std::thread::sleep(std::time::Duration::from_secs(1));
    let pitr_time_1 = run_sql(
        &*interactor,
        "SELECT to_char(clock_timestamp(), 'YYYY-MM-DD HH24:MI:SS');",
    );
    std::thread::sleep(std::time::Duration::from_secs(1));
    println!("Stored PITR time 1: {}", pitr_time_1);

    println!("Step 10: create incremental backup #1");
    let incr_backup_1 = crane::commands::postgres::run_backup_cmd(
        config_path,
        "incr",
        crane::server_interactor::get_interactor,
    )
    .expect("Incremental backup 1 failed");
    assert_eq!(incr_backup_1.backup_type, "INCR");

    println!("Step 11: insert 4 to table");
    run_sql(
        &*interactor,
        "INSERT INTO integrated_test_table VALUES (4);",
    );

    // Store time after inserting 4
    std::thread::sleep(std::time::Duration::from_secs(1));
    let pitr_time_2 = run_sql(
        &*interactor,
        "SELECT to_char(clock_timestamp(), 'YYYY-MM-DD HH24:MI:SS');",
    );
    std::thread::sleep(std::time::Duration::from_secs(1));
    println!("Stored PITR time 2: {}", pitr_time_2);

    println!("Step 12: create incremental backup #2");
    let incr_backup_2 = crane::commands::postgres::run_backup_cmd(
        config_path,
        "incr",
        crane::server_interactor::get_interactor,
    )
    .expect("Incremental backup 2 failed");
    assert_eq!(incr_backup_2.backup_type, "INCR");

    println!("Step 13: delete table");
    run_sql(&*interactor, "DROP TABLE integrated_test_table;");

    println!("Step 14: assert table is deleted");
    let exists = run_sql(
        &*interactor,
        "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = 'integrated_test_table');",
    );
    assert_eq!(exists, "f");

    println!("Step 15: restore from incremental backup #1 with --pitr");
    crane::commands::postgres::run_restore_cmd(
        config_path,
        &incr_backup_1.id,
        None,
        Some(&pitr_time_1),
        crane::server_interactor::get_interactor,
    )
    .expect("PITR restore to PITR time 1 failed");

    // Reconnect after restore
    let interactor = crane::postgres_unit::helper::connect_to_node(
        &primary_node,
        &config,
        crane::server_interactor::get_interactor,
    )
    .expect("Failed to reconnect to primary node");

    println!("Step 16: assert table have 1,2,3 in table");
    let rows = run_sql(
        &*interactor,
        "SELECT id FROM integrated_test_table ORDER BY id;",
    );
    assert_eq!(rows, "1\n2\n3");

    // --- INCREMENTAL BACKUP TEST #2 ---
    println!("Step 17: delete table");
    run_sql(&*interactor, "DROP TABLE integrated_test_table;");

    println!("Step 18: assert table is deleted");
    let exists = run_sql(
        &*interactor,
        "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = 'integrated_test_table');",
    );
    assert_eq!(exists, "f");

    println!("Step 19: restore from incremental backup #2 with --pitr");
    crane::commands::postgres::run_restore_cmd(
        config_path,
        &incr_backup_2.id,
        None,
        Some(&pitr_time_1),
        crane::server_interactor::get_interactor,
    )
    .expect("PITR restore using backup 2 failed");

    // Reconnect after restore
    let interactor = crane::postgres_unit::helper::connect_to_node(
        &primary_node,
        &config,
        crane::server_interactor::get_interactor,
    )
    .expect("Failed to reconnect to primary node");

    println!("Step 20: assert table have 1,2,3 in table");
    let rows = run_sql(
        &*interactor,
        "SELECT id FROM integrated_test_table ORDER BY id;",
    );
    assert_eq!(rows, "1\n2\n3");
}
