## Get Postgres Status


```sh
crane postgres status
```

Expected output:
```log

HAProxy
Primary: vps1:5000
Backup: vps2:5000,vps3:5000

vps1 
Address: [PUBLIC_IP_ADDRESS]:[PORT]
Role: Leader
DB version: 17
Health: Healthy

vps2
Address: [PUBLIC_IP_ADDRESS]:[PORT]
Role: Follower
DB version: 17
Health: Healthy

```


## Promote/Demote node

- Promote `vps2` to leader:
```sh
crane postgres promote vps2
```

- Verify status:
  - `vps2` should return `pg_is_in_recovery() -> f`
  - `vps1` should return `pg_is_in_recovery() -> t`
  - HAProxy should route connections to `vps2`.

- Demote `vps2` to follower (after making `vps1` leader again):
```sh
crane postgres promote vps1
```


# BACKUP / RESTORE

## Backup

`crane postgres backup <full / incr>` backup postgres in full or incremental. 

## List Backups

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

