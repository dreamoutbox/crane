#[test]
fn test_restore_full() {
    let interactor = MockInteractor::new(vec![]);
    let s3_client = MockS3Client::new();

    let backup = BackupMetadata {
        id: "20251211152749155".to_string(),
        date: "2025-12-11".to_string(),
        time: "15:27:49".to_string(),
        backup_type: "FULL".to_string(),
        base: None,
        local_path: "/var/lib/postgresql/backups/20251211152749155".to_string(),
        s3_path: "crane1/backups/20251211152749155".to_string(),
    };

    // Populate MockS3 with files
    s3_client
        .put_object("backups/20251211152749155/base.tar", b"dummy base")
        .unwrap();
    s3_client
        .put_object(
            "backups/20251211152749155/backup_manifest",
            b"dummy manifest",
        )
        .unwrap();

    let result = run_restore(&interactor, &s3_client, "17", &backup, &[backup.clone()]);
    assert!(result.is_ok());

    let cmds = interactor.commands.borrow();
    // Verify stop postgres
    assert!(cmds.iter().any(|c| c.contains("systemctl stop postgresql")));
    // Verify rm main data dir
    assert!(
        cmds.iter()
            .any(|c| c.contains("rm -rf /var/lib/postgresql/17/main"))
    );
    // Verify tar extraction
    assert!(
        cmds.iter()
            .any(|c| c.contains("tar -xf") && c.contains("/var/lib/postgresql/17/main"))
    );

    // Verify start postgres
    assert!(
        cmds.iter()
            .any(|c| c.contains("start") && c.contains("/var/lib/postgresql/17/main"))
    );
}

#[test]
fn test_restore_incremental() {
    let interactor = MockInteractor::new(vec![]);
    let s3_client = MockS3Client::new();

    let full_backup = BackupMetadata {
        id: "20251211152749155".to_string(),
        date: "2025-12-11".to_string(),
        time: "15:27:49".to_string(),
        backup_type: "FULL".to_string(),
        base: None,
        local_path: "/var/lib/postgresql/backups/20251211152749155".to_string(),
        s3_path: "crane1/backups/20251211152749155".to_string(),
    };

    let incr_backup = BackupMetadata {
        id: "20251211152849281".to_string(),
        date: "2025-12-11".to_string(),
        time: "15:28:49".to_string(),
        backup_type: "INCR".to_string(),
        base: Some("20251211152749155".to_string()),
        local_path: "/var/lib/postgresql/backups/20251211152849281".to_string(),
        s3_path: "crane1/backups/20251211152849281".to_string(),
    };

    // Populate MockS3 with files
    s3_client
        .put_object("backups/20251211152749155/base.tar", b"dummy base")
        .unwrap();
    s3_client
        .put_object(
            "backups/20251211152749155/backup_manifest",
            b"dummy manifest",
        )
        .unwrap();
    s3_client
        .put_object("backups/20251211152849281/base.tar", b"dummy base incr")
        .unwrap();
    s3_client
        .put_object(
            "backups/20251211152849281/backup_manifest",
            b"dummy manifest incr",
        )
        .unwrap();

    let result = run_restore(
        &interactor,
        &s3_client,
        "17",
        &incr_backup,
        &[full_backup.clone(), incr_backup.clone()],
    );
    assert!(result.is_ok());

    let cmds = interactor.commands.borrow();
    // Verify stop postgres
    assert!(cmds.iter().any(|c| c.contains("systemctl stop postgresql")));
    // Verify pg_combinebackup command is executed
    assert!(cmds.iter().any(|c| {
        c.contains("pg_combinebackup")
            && c.contains("/var/lib/postgresql/backups/20251211152749155_extracted")
            && c.contains("/var/lib/postgresql/backups/20251211152849281_extracted")
            && c.contains("-o /var/lib/postgresql/backups/combined")
    }));

    // Verify start postgres
    assert!(
        cmds.iter()
            .any(|c| c.contains("start") && c.contains("/var/lib/postgresql/17/main"))
    );
}
