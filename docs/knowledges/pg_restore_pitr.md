# PostgreSQL Point-in-Time Recovery (PITR)

> Restore your database to any moment in the past — e.g., 1 second before someone ran `DELETE FROM orders`.

---

## How PITR Works

```
[Base Backup] ──── [WAL stream] ──── [WAL stream] ──── [target time] ──▶ STOP
     t=0              t=1h              t=2h               t=2h45m
```

PostgreSQL replays WAL (Write-Ahead Log) segments from the base backup forward, stopping at your target. **You need both**:

1. A base backup (from `pg_basebackup`)
2. A continuous WAL archive covering the period from that backup to your target time

Without WAL archives, PITR is impossible. No exceptions.

---

## Prerequisites

### 1. WAL Archiving Must Be Running (Before the Incident)

```ini
# postgresql.conf
wal_level = replica
archive_mode = on
archive_command = 'cp %p /var/lib/postgresql/wal_archive/%f'
# For remote: 'rsync -a %p backup-server:/wal_archive/%f'
```

Verify archiving is active:

```sql
SELECT * FROM pg_stat_archiver;
-- last_failed_wal should be NULL, last_archived_time should be recent
```

### 2. You Have a Base Backup Taken BEFORE the Incident

```bash
# The base backup must predate the incident
ls -la /backup/
# e.g.: 20240601_020000_full/   ← taken at 02:00, incident at 14:32
```

---

## Scenario: Recover From Accidental DELETE

```
02:00  →  Full base backup taken
14:32  →  Someone runs: DELETE FROM orders WHERE 1=1;  ← disaster
14:33  →  You notice. You want to restore to 14:31:59.
```

---

## Step 1: Find the Target Time

### Option A — You know the approximate time

```sql
-- Check when the table was last modified (if still accessible)
SELECT now();  -- note current time, work backwards
```

### Option B — Parse WAL logs to find exact transaction time

```bash
# Install pg_waldump (comes with PostgreSQL)
pg_waldump \
  --path=/var/lib/postgresql/wal_archive \
  --start=0/1000000 \          # adjust to known WAL range
  --timeline=1 \
  | grep -i "delete\|COMMIT" \
  | tail -50
```

Output looks like:
```
rmgr: Heap        len (rec/tot):     59/    59, tx:        1842, lsn: 0/3A1F0028,
  prev 0/3A1F0000, desc: DELETE off 1, blkref #0: rel 1663/16384/16401 blk 0
rmgr: Transaction len (rec/tot):     46/    46, tx:        1842, lsn: 0/3A1F0090,
  prev 0/3A1F0028, desc: COMMIT 2024-06-01 14:32:17.843201 UTC   ← exact time
```

Your target: **`2024-06-01 14:32:17 UTC`** → restore to **`2024-06-01 14:32:16 UTC`** (1 second before).

### Option C — Use transaction ID (most precise)

```sql
-- If you can query before shutdown:
SELECT xmin, * FROM orders LIMIT 5;   -- note the xmin (txid) values
-- Restore to: recovery_target_xid = '<last good txid>'
```

---

## Step 2: Stop PostgreSQL

```bash
systemctl stop postgresql

# Confirm it's fully stopped
pg_lsclusters       # Debian/Ubuntu
# or
pg_ctl status -D /var/lib/postgresql/data
```

---

## Step 3: Prepare the Data Directory

```bash
PGDATA=/var/lib/postgresql/data
BACKUP_DIR=/backup/20240601_020000_full

# Back up current (broken) state — optional but recommended
mv "$PGDATA" "${PGDATA}_broken_$(date +%Y%m%d_%H%M%S)"
mkdir -p "$PGDATA"

# Restore base backup
# Tar format:
tar -xf "$BACKUP_DIR/base.tar.gz" -C "$PGDATA/"

# If WAL was streamed into pg_wal.tar.gz:
tar -xf "$BACKUP_DIR/pg_wal.tar.gz" -C "$PGDATA/pg_wal/"

# Fix permissions
chown -R postgres:postgres "$PGDATA"
chmod 700 "$PGDATA"
```

---

## Step 4: Configure Recovery Parameters

### PostgreSQL 12+ (recovery.signal + postgresql.conf)

```bash
# Add recovery settings to postgresql.conf
cat >> "$PGDATA/postgresql.conf" <<'EOF'

#----------------------
# PITR Recovery Config
#----------------------
restore_command = 'cp /var/lib/postgresql/wal_archive/%f %p'

# Stop recovery at this timestamp (1 second before the DELETE)
recovery_target_time = '2024-06-01 14:32:16 UTC'

# What to do when target is reached:
# 'promote'  → open for writes immediately (use this for most cases)
# 'pause'    → pause so you can inspect before promoting
# 'shutdown' → stop server (useful for verification)
recovery_target_action = 'promote'

# Inclusive: replay transactions UP TO AND INCLUDING the target time
# (usually leave as default 'on')
recovery_target_inclusive = on
EOF

# Create the recovery signal file (triggers recovery mode)
touch "$PGDATA/recovery.signal"
```

### PostgreSQL 11 and earlier (recovery.conf)

```bash
cat > "$PGDATA/recovery.conf" <<'EOF'
restore_command = 'cp /var/lib/postgresql/wal_archive/%f %p'
recovery_target_time = '2024-06-01 14:32:16 UTC'
recovery_target_action = 'promote'
EOF
```

---

## Step 5: Start PostgreSQL and Monitor Recovery

```bash
systemctl start postgresql

# Watch recovery progress
tail -f /var/log/postgresql/postgresql-*.log
```

Expected log output:

```
LOG:  starting point-in-time recovery to 2024-06-01 14:32:16+00
LOG:  restored log file "000000010000000000000001" from archive
LOG:  redo starts at 0/1000028
LOG:  consistent recovery state reached at 0/1000100
LOG:  restored log file "000000010000000000000002" from archive
...
LOG:  recovery stopping before commit of transaction 1842, time 2024-06-01 14:32:17.843201+00
LOG:  pausing at the end of recovery   ← if action=pause
LOG:  database system is ready to accept read only connections
```

---

## Step 6: Verify Data Before Promoting

```bash
# Connect as read-only to verify data is correct
psql -U postgres -c "SELECT count(*) FROM orders;"
psql -U postgres -c "SELECT * FROM orders LIMIT 10;"
```

If data looks wrong (stopped too early/late):

```bash
# Shut down and adjust recovery_target_time, then restart
systemctl stop postgresql
# Edit postgresql.conf, adjust the time
# Restore base backup again (recovery is destructive — you need a fresh copy)
systemctl start postgresql
```

> ⚠️ **You can only attempt recovery once per base backup restore.** Once WAL replay starts, the data directory is modified. Always keep your base backup intact and restore from scratch if you need to retry with a different target time.

---

## Step 7: Promote to Writable

If you used `recovery_target_action = 'pause'`:

```bash
# After verifying data is correct:
psql -U postgres -c "SELECT pg_promote();"
# or
pg_ctl promote -D /var/lib/postgresql/data
```

If you used `recovery_target_action = 'promote'`, it's already writable after recovery completes.

---

## Recovery Target Options (Cheat Sheet)

| Parameter | Type | Example | Use When |
|---|---|---|---|
| `recovery_target_time` | timestamp | `'2024-06-01 14:32:16 UTC'` | You know the approximate time |
| `recovery_target_xid` | txid | `'1842'` | You have the transaction ID |
| `recovery_target_lsn` | LSN | `'0/3A1F0028'` | You have the WAL position |
| `recovery_target_name` | label | `'before_migration'` | You created a named restore point |
| `recovery_target` | string | `'immediate'` | Restore to end of base backup only |

### Creating Named Restore Points (Proactive)

```sql
-- Run this BEFORE risky operations (migrations, bulk deletes, etc.)
SELECT pg_create_restore_point('before_delete_orders');
-- Then recover with:
-- recovery_target_name = 'before_delete_orders'
```

This is the safest approach — no guessing timestamps.

---

## Restoring to a Specific LSN (Most Precise)

```bash
# Find the LSN just before the bad transaction from pg_waldump output:
# desc: COMMIT 2024-06-01 14:32:17... lsn: 0/3A1F0090

# Set target to the LSN of the bad COMMIT:
cat >> "$PGDATA/postgresql.conf" <<'EOF'
restore_command = 'cp /var/lib/postgresql/wal_archive/%f %p'
recovery_target_lsn = '0/3A1F0090'
recovery_target_inclusive = off    # ← EXCLUDE this LSN (don't replay the DELETE)
recovery_target_action = 'promote'
EOF
```

`recovery_target_inclusive = off` with an LSN target means: replay everything **up to but not including** that transaction. Most surgical option.

---

## Incremental Backup + PITR (PG17+)

When restoring from an incremental backup chain, combine first, then follow the same PITR steps:

```bash
# Step 1: Combine backup chain into a full backup
pg_combinebackup \
  /backup/20240601_000000_full \
  /backup/20240601_060000_incr1 \
  /backup/20240601_120000_incr2 \
  -o /restore/combined

# Step 2: Use /restore/combined as your BACKUP_DIR
# Then follow Steps 2–7 above exactly as written
tar -xf /restore/combined/base.tar.gz -C "$PGDATA/"
```

---

## Sanity Checklist

```
[ ] archive_mode = on was set BEFORE the incident
[ ] WAL archive directory has files covering base backup → incident time
[ ] Base backup predates the incident
[ ] Base backup is intact (not overwritten)
[ ] recovery_target_time is in UTC (or matches server timezone)
[ ] recovery.signal file exists (PG12+)
[ ] postgresql.conf has restore_command pointing to correct archive path
[ ] Data directory permissions are 700, owned by postgres
[ ] Verified data before promoting (used recovery_target_action = 'pause')
```

---

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| `requested WAL segment has already been removed` | WAL not archived, was recycled | Enable `archive_mode` + verify `archive_command` ran |
| Recovery stops too early | `recovery_target_inclusive = on` with exact bad txn time | Use `recovery_target_lsn` with `inclusive = off` |
| Recovery stops too late | Target time after the bad txn | Subtract more time from `recovery_target_time` |
| `recovery.signal` ignored | PG11 or earlier | Use `recovery.conf` instead |
| Server won't start after restore | Bad permissions on PGDATA | `chmod 700 $PGDATA && chown -R postgres:postgres $PGDATA` |
| `restore_command` fails silently | Wrong path or WAL file missing | Test manually: `cp /wal_archive/000000010000000000000001 /tmp/test` |
