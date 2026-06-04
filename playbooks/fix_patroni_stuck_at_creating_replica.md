# Patroni — Stuck "creating replica" Playbook

**Scope**: Node stuck in `creating replica` state in `patronictl list` very long.  
**Target**: Coding agent. Execute commands in order. Follow conditional branches. Apply only the fixes that match findings.

## The problem.

when user run `cargo run -- -f demo/crane.toml deploy` the user got stuck at `Polling PostgreSQL cluster health...` very long.

---

## DEBUG Guidelines


- the patroni config & setup code at `src/postgres_unit/patroni.rs`

- the etcd config & setup code at `src/etcd_unit/etcd.rs`

- for debug we make the crane write debug patroni output at `./patroni_vps(VPS NODE NUMBER HERE).yaml`

- use `docker exec vps(VPS NODE NUMBER HERE) <command>` to run command in a vps server

- please redirect debug command output to a file. for example if you run step "3.3 Check wal_senders capacity". write the debug command to redirect to file "debug_output/3.3 Check wal_senders capacity_vps(VPS NODE NUMBER HERE).log" and read it from there.

- use this command to reproduce the problem:

```sh
docker compose -f 'docker-compose.dev.yml' down && \
docker compose -f 'docker-compose.dev.yml' up -d --build && clear && \
cargo run -- -f demo/crane.toml deploy
```

- use `logpg (VPS NODE NUMBER HERE)` to fast view patroni debug log.
- use `logetcd (VPS NODE NUMBER HERE)` to fast view etcd logs.
- use `ptl` for fast view patronictl list.

---

## Variables (set before running)

```bash
PATRONI_CFG="/etc/patroni/patroni.yml"          # adjust if different
PGDATA="/var/lib/postgresql/17/main"             # match data_dir in patroni.yml
PGBIN="/usr/lib/postgresql/17/bin"               # match bin_dir in patroni.yml
STUCK_NODE="vps2"                                # node showing 'creating replica'
LEADER_HOST="10.0.0.13"                          # current leader IP
LEADER_PORT="5432"
REPLICATION_USER="replicator"
PATRONI_SCOPE="postgres-cluster"
WAL_ARCHIVE_DIR="/var/lib/postgresql/wal_archive"
```

---

## Phase 1 — Cluster State

Run on **any node**.

```bash
patronictl -c $PATRONI_CFG list
```

**Expected healthy output**: all members show `streaming` or `running`. Any node showing `creating replica` for >5 min → proceed.

```bash
# How long has the stuck node been in this state?
patronictl -c $PATRONI_CFG list | grep "creating replica"

# Check DCS (etcd) health
etcdctl --endpoints=http://10.0.0.11:2379,http://10.0.0.12:2379,http://10.0.0.13:2379 endpoint health
etcdctl --endpoints=http://10.0.0.11:2379,http://10.0.0.12:2379,http://10.0.0.13:2379 endpoint status
```

**If etcd shows unhealthy members** → fix etcd cluster first. Do not proceed with Patroni fixes until etcd quorum is restored.

---

## Phase 2 — Check pg_basebackup on the Stuck Node

Run on the **stuck node** (`vps2`).

### 2.1 Is pg_basebackup actually running?

```bash
ps aux | grep -E 'pg_basebackup|postgres' | grep -v grep
```

**Branch A — pg_basebackup process IS running**:

```bash
# Watch data directory grow (confirms transfer is progressing)
watch -n 5 'du -sh $PGDATA'

# Check basebackup progress (PostgreSQL 14+)
psql -h $LEADER_HOST -p $LEADER_PORT -U $REPLICATION_USER \
  -c "SELECT pid, phase, backup_total, backup_streamed, tablespaces_total, tablespaces_streamed FROM pg_stat_progress_basebackup;"

# Check network throughput to leader
iftop -i eth0 -f "host $LEADER_HOST"
# or
nload eth0
```

If `du -sh` is growing → basebackup is running but slow (large database or slow network). **No fix needed, just wait.** Optionally apply [Fix F3] to prevent future slowness.

If `du -sh` is NOT growing for >5 min → pg_basebackup is hung. Apply **[Fix F1]**.

**Branch B — pg_basebackup is NOT running**:

```bash
# Check if postgres is running in recovery
ps aux | grep postgres | grep -v grep

# Check patroni log for last error
journalctl -u patroni -n 100 --no-pager | grep -E 'ERROR|WARNING|FATAL|basebackup'
# or if using file log:
tail -100 /var/log/patroni/patroni.log | grep -E 'ERROR|WARNING|FATAL|basebackup'
```

If patroni log shows `pg_basebackup` exited with error → go to **Phase 3**.
If patroni log shows no recent activity → apply **[Fix F1]** (restart patroni).

---

## Phase 3 — Check WAL Archiving on the Leader

Run on the **leader node** (`vps3`).

### 3.1 Is the WAL archive directory accessible?

```bash
ls -la $WAL_ARCHIVE_DIR
```

**If directory does not exist**:
```bash
# This is the cause — archive_command (cp) fails on every WAL segment
# Apply Fix F2 immediately
```
→ Apply **[Fix F2]**.

**If directory exists**, check for recent archive failures:

```bash
# Look for archive failures in PostgreSQL log
find $PGDATA/log -name "*.csv" | xargs grep -l "archive" | tail -1 | xargs tail -50
# or plain log:
grep -i "archive" $PGDATA/log/postgresql*.log | tail -20
```

If output contains `archive command failed` or `archive_command returned exit code` → archive is broken. Apply **[Fix F2]**.

### 3.2 Check replication slots

```bash
psql -U postgres -c "
SELECT slot_name, active, restart_lsn, wal_status,
       pg_size_pretty(pg_wal_lsn_diff(pg_current_wal_lsn(), restart_lsn)) AS lag
FROM pg_replication_slots;
"
```

**If stuck node's slot shows `active = false` with very large lag or `wal_status = 'lost'`**:
```bash
# The slot is stale and may be blocking WAL cleanup
# Apply Fix F4
```
→ Apply **[Fix F4]**.

**If stuck node has NO slot at all** and `use_slots: true` is set → slot creation may have failed. Apply **[Fix F1]** to let Patroni retry.

### 3.3 Check wal_senders capacity

```bash
psql -U postgres -c "SHOW max_wal_senders;"
psql -U postgres -c "SELECT count(*) FROM pg_stat_replication;"
psql -U postgres -c "
SELECT pid, usename, application_name, client_addr, state, sync_state
FROM pg_stat_replication;
"
```

**If `count(*) >= max_wal_senders`** → no wal_sender slots available for the basebackup. Apply **[Fix F5]**.

### 3.4 Check if leader can reach stuck node

```bash
pg_isready -h $LEADER_HOST -p $LEADER_PORT
# From leader, test connectivity to stuck node's Patroni REST API
curl -s http://10.0.0.12:8008/health | python3 -m json.tool
```

---

## Phase 4 — Check Patroni Config on Stuck Node

Run on the **stuck node** (`vps2`).

### 4.1 Validate WAL method used for pg_basebackup

```bash
grep -A 10 'basebackup' $PATRONI_CFG
```

**If `wal-method: stream` is absent** from the basebackup section → Patroni may be using `--wal-method=fetch`. With `archive_mode=on` and a failing `archive_command`, pg_basebackup will block at `pg_backup_stop()` waiting for WAL archiving to complete — indefinitely.

→ Apply **[Fix F3]**.

### 4.2 Check DCS timeout config

```bash
grep -E 'ttl|loop_wait|retry_timeout' $PATRONI_CFG
```

**Dangerous pattern** (causes "Loop time exceeded" and false failovers):
```yaml
ttl: 10         # < 20 is too low
loop_wait: 2    # < 5 is too aggressive
retry_timeout: 3  # should be >= loop_wait
```

→ Apply **[Fix F6]** (safe to apply, requires patroni restart).

### 4.3 Check disk space on stuck node

```bash
df -h $PGDATA
df -h $WAL_ARCHIVE_DIR
```

**If disk is >90% full** → pg_basebackup will fail partway through. Free disk space first. At minimum, `PGDATA` needs enough space to hold the full database copy.

---

## Phase 5 — Connectivity Check

Run on the **stuck node** (`vps2`).

```bash
# Can stuck node connect to leader for replication?
psql -h $LEADER_HOST -p $LEADER_PORT -U $REPLICATION_USER \
  -c "IDENTIFY_SYSTEM;" replication=1

# Check pg_hba.conf allows replication from stuck node's IP
psql -h $LEADER_HOST -U postgres \
  -c "SELECT type, database, user_name, address, auth_method FROM pg_hba_file_rules WHERE database = '{replication}';"
```

**If `psql` connection fails** → network or pg_hba issue.

```bash
# Test raw TCP connectivity
nc -zv $LEADER_HOST $LEADER_PORT

# Check firewall
iptables -L -n | grep $LEADER_PORT
```

**If TCP connection is refused or times out** → firewall or network misconfiguration. Fix firewall rules before applying any Patroni fix.

---

## Fixes

---

### Fix F1 — Restart Patroni on stuck node

**Use when**: pg_basebackup is not running, or Patroni is stuck with no active process.

```bash
# On the stuck node (vps2)
sudo systemctl stop patroni

# Wipe the data directory so Patroni starts fresh
sudo -u postgres rm -rf $PGDATA/*

# Start patroni
sudo systemctl start patroni

# Tail the log immediately
journalctl -u patroni -f --no-pager
```

**Verify**:
```bash
watch -n 3 "patronictl -c $PATRONI_CFG list"
```

Expected: node transitions from `creating replica` → `streaming` within minutes depending on DB size.

---

### Fix F2 — Create WAL archive directory on all nodes

**Use when**: `ls $WAL_ARCHIVE_DIR` fails OR archive_command errors appear in PG logs.

Run on **all nodes**:

```bash
sudo -u postgres mkdir -p $WAL_ARCHIVE_DIR
sudo chmod 700 $WAL_ARCHIVE_DIR

# Verify
ls -la $WAL_ARCHIVE_DIR
```

If using a remote archive destination (S3, NFS), verify mount/credentials separately.

To make `archive_command` self-healing (update `patroni.yml` on all nodes):

```yaml
postgresql:
  parameters:
    archive_command: "mkdir -p /var/lib/postgresql/wal_archive && cp %p /var/lib/postgresql/wal_archive/%f"
```

Reload config:
```bash
patronictl -c $PATRONI_CFG reload $PATRONI_SCOPE
```

---

### Fix F3 — Add explicit wal-method: stream to basebackup config

**Use when**: `wal-method: stream` is absent from `basebackup` section in `patroni.yml`.

Edit `patroni.yml` on **all nodes** (must be identical under `bootstrap.basebackup`):

```yaml
bootstrap:
  basebackup:
    - checkpoint: fast
    - no-verify-checksums
    - wal-method: stream     # ← add this line
```

This config change only affects the next `pg_basebackup` invocation. No restart needed for already-running nodes, but the stuck node needs a restart to re-trigger basebackup:

```bash
# Only on stuck node
sudo systemctl restart patroni
```

---

### Fix F4 — Drop stale replication slot on leader

**Use when**: Stuck node's slot exists on leader with `active = false` and large lag or `wal_status = 'lost'`.

Run on the **leader** (`vps3`):

```bash
# Replace vps2 with the actual slot name (usually matches node name)
psql -U postgres -c "SELECT pg_drop_replication_slot('vps2');"
```

Patroni will recreate the slot when the replica reconnects. Then restart Patroni on the stuck node:

```bash
# On stuck node (vps2)
sudo systemctl restart patroni
```

---

### Fix F5 — Increase max_wal_senders

**Use when**: `count(*) FROM pg_stat_replication` equals `max_wal_senders`.

`pg_basebackup --wal-method=stream` requires **2 wal_sender slots** (one for data, one for WAL streaming). Plus 1 per existing streaming replica.

Minimum required: `(number of replicas) + (number of concurrent basebackups × 2) + 1 spare`

Edit `patroni.yml` on all nodes:

```yaml
bootstrap:
  dcs:
    postgresql:
      parameters:
        max_wal_senders: 15    # increase from 10
        max_replication_slots: 15
```

Apply without restart:

```bash
patronictl -c $PATRONI_CFG edit-config
# or reload
patronictl -c $PATRONI_CFG reload $PATRONI_SCOPE
```

Verify it took effect:

```bash
psql -U postgres -h $LEADER_HOST -c "SHOW max_wal_senders;"
```

---

### Fix F6 — Fix aggressive DCS timeout settings

**Use when**: Logs show `Loop time exceeded` warnings repeatedly, or false failovers occur during basebackup.

Edit `patroni.yml` on **all nodes** under `bootstrap.dcs`:

```yaml
bootstrap:
  dcs:
    ttl: 30              # was 10 — minimum: loop_wait × 2
    loop_wait: 10        # was 2
    retry_timeout: 10    # was 3 — must be >= loop_wait
```

**Rule of thumb**:
- `retry_timeout` ≥ `loop_wait`  
- `ttl` ≥ `loop_wait × 2` (preferably `× 3`)

Apply to DCS (takes effect cluster-wide without restart):

```bash
patronictl -c $PATRONI_CFG edit-config --force
```

Restart Patroni on all nodes **one at a time**, waiting for `streaming` status before moving to the next:

```bash
# On each node sequentially
sudo systemctl restart patroni
sleep 10
patronictl -c $PATRONI_CFG list
```

---

## Verification Checklist

After applying any fix, confirm:

```bash
# 1. All nodes healthy
patronictl -c $PATRONI_CFG list
# Expected: all nodes streaming or running, no 'creating replica'

# 2. Replication lag is 0
patronictl -c $PATRONI_CFG list | grep -E 'Lag|Replay'

# 3. No archive errors on leader
psql -U postgres -h $LEADER_HOST \
  -c "SELECT archived_count, failed_count, last_failed_wal, last_failed_time FROM pg_stat_archiver;"
# failed_count should not be increasing

# 4. Replication slots all active
psql -U postgres -h $LEADER_HOST \
  -c "SELECT slot_name, active, wal_status FROM pg_replication_slots;"
# All slots should show active = true, wal_status = 'reserved' or 'extended'

# 5. No Loop time exceeded in patroni logs
journalctl -u patroni --since "5 minutes ago" --no-pager | grep "Loop time exceeded"
# Should be empty
```

---

## Decision Tree Summary

```
stuck in 'creating replica'
│
├── pg_basebackup running AND du growing?
│   └── YES → just wait (large DB). Optionally apply F3.
│
├── pg_basebackup running AND du NOT growing?
│   └── apply F1 (restart)
│
├── pg_basebackup NOT running?
│   ├── check patroni log for error
│   ├── wal_archive dir missing?        → F2 then F1
│   ├── stale replication slot?         → F4 then F1
│   ├── max_wal_senders exhausted?      → F5 then F1
│   ├── no wal-method: stream in cfg?   → F3 then F1
│   ├── disk full?                      → free space then F1
│   └── no specific error found?        → F1
│
└── DCS unhealthy?
    └── fix etcd first, then F6, then F1
```

---

## Anti-patterns to Prevent Recurrence

| Anti-pattern | Risk | Fix |
|---|---|---|
| `wal-method` not explicit in basebackup | Hangs if archive broken | Add `wal-method: stream` |
| `archive_command` with `cp` to non-existent dir | archive fails silently | Create dir or use `mkdir -p &&` prefix |
| `ttl: 10 / loop_wait: 2 / retry_timeout: 3` | False failovers under load | Apply Fix F6 |
| `use_slots: true` without monitoring slot lag | WAL disk exhaustion on leader | Set `maximum_lag_on_failover: 1048576` |
| `log_min_duration_statement: 0` in production | Log I/O overhead | Set to `1000` (1s) or remove |
| No `--checkpoint=fast` equivalent during basebackup | Waits for next scheduled checkpoint | Keep `checkpoint: fast` |