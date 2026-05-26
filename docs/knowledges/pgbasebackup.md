# pg_basebackup: Full & Incremental Backup/Restore

> Incremental backup requires **PostgreSQL 17+**. Full backup works on PG 10+.

---

## Prerequisites

### PostgreSQL Configuration (`postgresql.conf`)

```ini
wal_level = replica          # minimum for backup
archive_mode = on            # required for WAL archiving
archive_command = 'cp %p /var/lib/postgresql/wal_archive/%f'
max_wal_senders = 3          # at least 1 for backup
```

### pg_hba.conf

```
# Allow replication connection for backup user
local   replication   backupuser                    trust
host    replication   backupuser  127.0.0.1/32      scram-sha-256
host    replication   backupuser  192.168.1.0/24    scram-sha-256
```

### Backup User

```sql
CREATE ROLE backupuser WITH REPLICATION LOGIN PASSWORD 'strongpassword';
-- PG17+: also needs pg_read_server_files for incremental manifest reads
GRANT pg_read_server_files TO backupuser;
```

---

## Full Backup

### Basic Full Backup

```bash
pg_basebackup \
  -h localhost \
  -U backupuser \
  -D /backup/base \
  --format=tar \          # tar | plain
  --gzip \                # compress (use --compress=lz4 for PG15+)
  --checkpoint=fast \     # don't wait for scheduled checkpoint
  --wal-method=stream \   # stream WAL during backup (safest)
  --progress \
  --verbose
```

### Recommended Production Full Backup

```bash
BACKUP_DIR="/backup/$(date +%Y%m%d_%H%M%S)_full"

pg_basebackup \
  -h localhost \
  -U backupuser \
  -D "$BACKUP_DIR" \
  --format=tar \
  --compress=lz4 \        # faster than gzip, PG15+
  --checkpoint=fast \
  --wal-method=stream \
  --manifest-checksums=sha256 \   # integrity verification, PG13+
  --progress \
  --no-password           # use .pgpass or PGPASSWORD env
```

> **`--wal-method` options:**
> - `stream` — streams WAL concurrently; requires 2 wal_senders; safest
> - `fetch` — fetches WAL at end; risk of WAL recycled before backup completes
> - `none` — no WAL included; you must archive WAL separately

### Verify Backup Manifest (PG13+)

```bash
pg_verifybackup "$BACKUP_DIR"
```

---

## Incremental Backup (PostgreSQL 17+)

Incremental backups reference a prior backup's manifest. Chain: `full → incr1 → incr2 → ...`

### First Incremental (from full)

```bash
pg_basebackup \
  -h localhost \
  -U backupuser \
  -D /backup/20240601_120000_incr1 \
  --incremental=/backup/20240601_000000_full/backup_manifest \
  --format=tar \
  --compress=lz4 \
  --checkpoint=fast \
  --wal-method=stream \
  --manifest-checksums=sha256
```

### Chained Incremental (from prior incremental)

```bash
pg_basebackup \
  -h localhost \
  -U backupuser \
  -D /backup/20240601_180000_incr2 \
  --incremental=/backup/20240601_120000_incr1/backup_manifest \
  --format=tar \
  --compress=lz4 \
  --checkpoint=fast \
  --wal-method=stream \
  --manifest-checksums=sha256
```

### Verify Each Incremental

```bash
pg_verifybackup /backup/20240601_120000_incr1
pg_verifybackup /backup/20240601_180000_incr2
```

---

## Backup Strategy Example

```
/backup/
├── 20240601_000000_full/        ← full every Sunday
│   └── backup_manifest
├── 20240601_060000_incr1/       ← incremental every 6h
│   └── backup_manifest
├── 20240601_120000_incr2/
│   └── backup_manifest
└── 20240601_180000_incr3/
    └── backup_manifest
```

---

## Restore: From Full Backup

### 1. Stop PostgreSQL

```bash
systemctl stop postgresql
```

### 2. Extract Backup

```bash
PGDATA=/var/lib/postgresql/data

# Clear existing data dir (DESTRUCTIVE)
rm -rf "$PGDATA"/*

# Plain format
cp -r /backup/20240601_000000_full/* "$PGDATA/"

# Tar format
tar -xf /backup/20240601_000000_full/base.tar.gz -C "$PGDATA/"
# If WAL was streamed separately:
mkdir -p "$PGDATA/pg_wal"
tar -xf /backup/20240601_000000_full/pg_wal.tar.gz -C "$PGDATA/pg_wal/"

chown -R postgres:postgres "$PGDATA"
chmod 700 "$PGDATA"
```

### 3. Configure Recovery (Point-in-Time)

```bash
# PG12+: use recovery parameters in postgresql.conf
cat >> "$PGDATA/postgresql.conf" <<EOF
restore_command = 'cp /var/lib/postgresql/wal_archive/%f %p'
# Optional PITR:
# recovery_target_time = '2024-06-01 17:30:00'
# recovery_target_action = 'promote'
EOF

# Create recovery signal file
touch "$PGDATA/recovery.signal"   # PG12+
# PG11 and earlier: use recovery.conf instead
```

### 4. Start PostgreSQL

```bash
systemctl start postgresql

# Monitor recovery
tail -f /var/log/postgresql/postgresql-*.log
# Look for: "database system is ready to accept connections"
```

### 5. Promote (if using PITR target)

```bash
# If recovery_target_action != 'promote', manually promote:
pg_ctl promote -D "$PGDATA"
# or:
psql -c "SELECT pg_promote();"
```

---

## Restore: From Incremental Backup (PG17+)

Incremental restore requires **combining** the full + all incrementals using `pg_combinebackup` before restoring. You cannot apply incrementals directly to a live data directory.

### 1. Combine Backup Chain

```bash
# Syntax: pg_combinebackup <full> [<incr1> <incr2> ...] -o <output_dir>

pg_combinebackup \
  /backup/20240601_000000_full \
  /backup/20240601_060000_incr1 \
  /backup/20240601_120000_incr2 \
  /backup/20240601_180000_incr3 \
  -o /restore/combined

# Verify the combined backup
pg_verifybackup /restore/combined
```

> **Order matters**: must be chronological — full first, then each incremental in sequence.

### 2. Restore from Combined Output

Follow the same steps as full backup restore, using `/restore/combined` as the source:

```bash
systemctl stop postgresql

rm -rf /var/lib/postgresql/data/*

# Tar format output from pg_combinebackup:
tar -xf /restore/combined/base.tar.gz -C /var/lib/postgresql/data/
tar -xf /restore/combined/pg_wal.tar.gz -C /var/lib/postgresql/data/pg_wal/

chown -R postgres:postgres /var/lib/postgresql/data
chmod 700 /var/lib/postgresql/data

cat >> /var/lib/postgresql/data/postgresql.conf <<EOF
restore_command = 'cp /var/lib/postgresql/wal_archive/%f %p'
EOF

touch /var/lib/postgresql/data/recovery.signal

systemctl start postgresql
```

---

## Automation Script Skeleton

```bash
#!/usr/bin/env bash
set -euo pipefail

BACKUP_ROOT="/backup"
PG_HOST="localhost"
PG_USER="backupuser"
PGPASSWORD="strongpassword"
export PGPASSWORD

LAST_MANIFEST_FILE="/var/lib/postgresql/last_backup_manifest"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)

if [[ ! -f "$LAST_MANIFEST_FILE" ]]; then
  # Full backup
  BACKUP_DIR="$BACKUP_ROOT/${TIMESTAMP}_full"
  pg_basebackup -h "$PG_HOST" -U "$PG_USER" -D "$BACKUP_DIR" \
    --format=tar --compress=lz4 --checkpoint=fast \
    --wal-method=stream --manifest-checksums=sha256
  echo "$BACKUP_DIR/backup_manifest" > "$LAST_MANIFEST_FILE"
else
  # Incremental backup
  LAST_MANIFEST=$(cat "$LAST_MANIFEST_FILE")
  BACKUP_DIR="$BACKUP_ROOT/${TIMESTAMP}_incr"
  pg_basebackup -h "$PG_HOST" -U "$PG_USER" -D "$BACKUP_DIR" \
    --incremental="$LAST_MANIFEST" \
    --format=tar --compress=lz4 --checkpoint=fast \
    --wal-method=stream --manifest-checksums=sha256
  echo "$BACKUP_DIR/backup_manifest" > "$LAST_MANIFEST_FILE"
fi

pg_verifybackup "$BACKUP_DIR"
echo "Backup complete: $BACKUP_DIR"
```

---

## Common Pitfalls

| Issue | Cause | Fix |
|---|---|---|
| `could not connect to server` | No replication slot/hba entry | Check `pg_hba.conf`, role has `REPLICATION` |
| `WAL file not found` during restore | `archive_mode` was off or WAL recycled | Use `--wal-method=stream`; ensure archiving |
| Incremental backup larger than expected | `summarize_wal` not enabled | Set `summarize_wal = on` in `postgresql.conf` (PG17+) |
| `pg_combinebackup` manifest mismatch | Chain broken / wrong order | Incrementals must form an unbroken chain from the same full |
| Recovery loops / doesn't promote | Missing `recovery.signal` or wrong target | Verify signal file exists; check `recovery_target_action` |

### PG17 Performance Tip

Enable WAL summarization to make incremental backups faster and smaller:

```ini
# postgresql.conf
summarize_wal = on
```

Without this, PostgreSQL falls back to scanning all data blocks to detect changes, making incrementals slow.

---

## Quick Reference

```
Full backup:        pg_basebackup -D <dir> --format=tar --wal-method=stream
Incremental:        pg_basebackup -D <dir> --incremental=<prev_manifest> ...
Verify:             pg_verifybackup <backup_dir>
Combine for restore: pg_combinebackup <full> [<incr>...] -o <output>
Restore signal:     touch $PGDATA/recovery.signal   (PG12+)
```
