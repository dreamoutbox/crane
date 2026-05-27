#[test]
fn test_backup_full() {
    let interactor = MockInteractor::new(vec!["20251211152749155 2025-12-11 15:27:49".to_string()]);
    let s3_client = MockS3Client::new();

    let result = run_backup(
        &interactor,
        &s3_client,
        "17",
        "full",
        "password123",
        "crane1",
        None,
    );
    assert!(result.is_ok());
    let meta = result.unwrap();
    assert_eq!(meta.backup_type, "FULL");
    assert!(meta.base.is_none());
    assert_eq!(meta.id, "20251211152749155");
    assert_eq!(meta.s3_path, "crane1/backups/20251211152749155");

    // Verify that pg_basebackup command was executed with the expected arguments
    let cmds = interactor.commands.borrow();
    assert!(cmds.iter().any(|c| {
        c.contains("pg_basebackup")
            && c.contains("-F t")
            && c.contains("-X s")
            && c.contains("-U replicator")
            && !c.contains("--incremental")
    }));

    // Verify that objects were uploaded to MockS3
    let objects = s3_client.objects.borrow();
    assert!(objects.contains_key("backups/20251211152749155/base.tar"));
    assert!(objects.contains_key("backups/20251211152749155/backup_manifest"));
    assert!(objects.contains_key("backups/20251211152749155/metadata.toml"));
    assert!(objects.contains_key("backups/registry.toml"));
}

#[test]
fn test_backup_incremental() {
    let interactor = MockInteractor::new(vec![
        "20251211152949394 2025-12-11 15:29:49".to_string(),
        "20251211153049555 2025-12-11 15:30:49".to_string(),
    ]);
    let s3_client = MockS3Client::new();

    // 1. Run full backup
    let result_full = run_backup(
        &interactor,
        &s3_client,
        "17",
        "full",
        "password123",
        "crane1",
        None,
    );
    assert!(
        result_full.is_ok(),
        "full backup failed: {:?}",
        result_full.err()
    );
    let meta_full = result_full.unwrap();

    // 2. Run incremental backup
    let result_incr = run_backup(
        &interactor,
        &s3_client,
        "17",
        "incr",
        "password123",
        "crane1",
        Some(&meta_full),
    );
    assert!(result_incr.is_ok());
    let meta_incr = result_incr.unwrap();

    assert_eq!(meta_incr.backup_type, "INCR");
    assert_eq!(meta_incr.base, Some(meta_full.id.clone()));
    assert_eq!(meta_incr.id, "20251211153049555");
    assert_eq!(meta_incr.s3_path, "crane1/backups/20251211153049555");

    // Verify that pg_basebackup was called with --incremental pointing to parent manifest
    let cmds = interactor.commands.borrow();
    assert!(cmds.iter().any(|c| {
        c.contains("pg_basebackup")
            && c.contains(
                "--incremental=/var/lib/postgresql/backups/20251211152949394/backup_manifest",
            )
    }));

    // Verify that since test -f returned 'no', it downloaded the parent manifest from S3 and uploaded it to VPS
    let files = interactor.files.borrow();
    assert!(files.contains_key("/var/lib/postgresql/backups/20251211152949394/backup_manifest"));
}
