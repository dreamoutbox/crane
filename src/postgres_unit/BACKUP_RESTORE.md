# BACKUP / RESTORE

## Backup

`crane postgres backup <full / incr>` backup postgres in full or incremental. 

## List

`crane postgres list` list all backups in current postgres cluster

Output:

```log
20251211152749155
Date: 2025-12-11
Time: 15:27:49
Type: FULL
LOCAL: /path/to/backup/20251211152749155
S3: bucket-name/path/to/backup/20251211152749155

20251211152849281
Date: 2025-12-11
Time: 15:28:49
Type: INCR
Base: 20251211152749155
LOCAL: /path/to/backup/20251211152849281
S3: bucket-name/path/to/backup/20251211152849281

20251211152949394
Date: 2025-12-11
Time: 15:29:49
Type: INCR
Base: 20251211152849281
LOCAL: /path/to/backup/20251211152949394
S3: bucket-name/path/to/backup/20251211152949394
```

## Restore

`crane postgres restore <id>` restore postgres from backup
