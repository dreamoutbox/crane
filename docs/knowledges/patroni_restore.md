# Guide: Restoring a Patroni Cluster from a pg_basebackup Full Backup
 

## Overview

This guide walks you through restoring a **Patroni-managed PostgreSQL cluster** using a full backup created with `pg_basebackup`. This procedure is useful in the following scenarios:

- Disaster recovery after data corruption or loss
- Migrating a cluster to new hardware
- Cloning a production cluster to a staging environment
- Recovering from accidental data deletion

The restore process follows a **safe, staged approach**:

1. Start only the **primary node first**
2. **Inspect and validate the data** with `psql` before bringing replicas online
3. Start **replica nodes** only after data is confirmed correct

This prevents propagating corrupted or incorrect data to replicas during the restore.

> ⚠️ **Warning:** This procedure will **replace all existing data** on the cluster nodes. Ensure you have verified your backup before proceeding.

---

## Prerequisites

- `pg_basebackup` backup archive (tar or plain format)
- Access to all Patroni cluster nodes via SSH
- `sudo` or `root` privileges on all nodes
- Patroni, PostgreSQL, and etcd/Consul/ZooKeeper (DCS) installed
- Backup of your current `patroni.yml` configuration file
- WAL archive access (if you need Point-in-Time Recovery)

### Tools Required

```bash
# Verify tools are available on all nodes
which patronictl
which patroni
which pg_basebackup
which psql
```

---

## Architecture Assumptions

This guide assumes the following cluster layout:

| Node | Role | IP |
|------|------|----|
| `node1` | Primary (leader) | `10.0.0.11` |
| `node2` | Replica | `10.0.0.12` |
| `node3` | Replica | `10.0.0.13` |
| `etcd` | DCS | `127.0.0.1:2379` (on every nodes) |

Patroni configuration file location: `/etc/patroni/patroni.yml`  
PostgreSQL data directory: `/var/lib/postgresql/data`  
Backup location: `/backup/pg_basebackup/`

---

## Step 1: Stop the Patroni Cluster

Stop Patroni on **all nodes** to prevent any writes or leadership changes during the restore process.

### On all nodes (node1, node2, node3):

```bash
# Stop Patroni service
sudo systemctl stop patroni

# Verify Patroni is stopped
sudo systemctl status patroni

# Also stop PostgreSQL if it is still running
sudo systemctl stop postgresql

# Verify PostgreSQL is not running
sudo -u postgres pg_ctl status -D /var/lib/postgresql/data
```

> 📝 **Note:** Make sure all nodes are stopped before proceeding. A running replica could interfere with the restore.

---

## Step 2: Verify the Backup

Before restoring, verify the integrity and contents of your backup.

```bash
# Navigate to backup directory
ls -lh /backup/pg_basebackup/

# If backup is in tar format, list the contents
tar -tvf /backup/pg_basebackup/base.tar.gz | head -50

# Check the backup label
tar -xOf /backup/pg_basebackup/base.tar.gz backup_label 2>/dev/null \
  || cat /backup/pg_basebackup/backup_label

# Example output of backup_label:
# START WAL LOCATION: 0/5000028 (file 000000010000000000000005)
# CHECKPOINT LOCATION: 0/5000060
# BACKUP METHOD: streamed
# BACKUP FROM: primary
# START TIME: 2024-01-15 10:00:00 UTC
# LABEL: pg_basebackup base backup
```

### Check available WAL files (if using WAL archiving):

```bash
ls -lh /backup/wal_archive/ | tail -20
```

> ✅ **Confirm:** Note the `START WAL LOCATION` and `START TIME` from the backup label. You will need this for recovery configuration.

---

## Step 3: Prepare All Nodes

Perform these steps on **all nodes** before restoring.

### 3.1 Back Up Existing Configuration Files

```bash
# Backup Patroni configuration
sudo cp /etc/patroni/patroni.yml /etc/patroni/patroni.yml.bak

# Backup PostgreSQL config files if they are outside the data directory
sudo cp /etc/postgresql/postgresql.conf /etc/postgresql/postgresql.conf.bak 2>/dev/null || true
sudo cp /etc/postgresql/pg_hba.conf /etc/postgresql/pg_hba.conf.bak 2>/dev/null || true
```

### 3.2 Clear the Existing Data Directory

Perform on **all nodes**:

```bash
sudo -u postgres bash

# Remove existing data directory contents
rm -rf /var/lib/postgresql/data/*

# Also remove hidden files
rm -rf /var/lib/postgresql/data/.[!.]*

# Verify it is empty
ls -la /var/lib/postgresql/data/

exit
```

> ⚠️ **Double-check** that you are removing the correct directory before running `rm -rf`.

---

## Step 4: Restore the Backup on the Primary Node

Perform this step **only on node1** (the node that will become the new primary).

### 4.1 Restore from Tar Format Backup

```bash
sudo -u postgres bash

# Extract the base backup
tar -xzf /backup/pg_basebackup/base.tar.gz \
    -C /var/lib/postgresql/data/

# If WAL files are in a separate tar (pg_basebackup creates pg_wal.tar)
tar -xzf /backup/pg_basebackup/pg_wal.tar.gz \
    -C /var/lib/postgresql/data/pg_wal/

echo "Base backup extracted successfully"
ls -la /var/lib/postgresql/data/

exit
```

### 4.2 Restore from Plain Format Backup

If your backup was taken in plain (non-tar) format:

```bash
sudo -u postgres bash

# Copy the backup directory to the data directory
rsync -av --progress /backup/pg_basebackup/ /var/lib/postgresql/data/

echo "Base backup copied successfully"

exit
```

### 4.3 Fix Permissions

```bash
sudo chown -R postgres:postgres /var/lib/postgresql/data/
sudo chmod 700 /var/lib/postgresql/data/

# Verify
ls -la /var/lib/postgresql/
```

---

## Step 5: Configure Recovery on the Primary Node

### 5.1 Remove Stale Signal and State Files

```bash
sudo -u postgres bash
cd /var/lib/postgresql/data/

# Remove any existing signal files
rm -f recovery.signal standby.signal

# Remove old Patroni dynamic state — Patroni will recreate this
rm -f patroni.dynamic.json

exit
```

### 5.2 Configure postgresql.conf

Ensure your `postgresql.conf` contains the settings Patroni requires:

```bash
sudo -u postgres tee -a /var/lib/postgresql/data/postgresql.conf << 'EOF'

# Patroni required settings
listen_addresses = '*'
port = 5432
wal_level = replica
max_wal_senders = 10
max_replication_slots = 10
hot_standby = on
wal_log_hints = on
EOF
```

### 5.3 Configure Recovery (Optional — for PITR)

If you need Point-in-Time Recovery, add restore settings:

```bash
sudo -u postgres tee -a /var/lib/postgresql/data/postgresql.conf << 'EOF'

# PITR settings — remove after successful recovery
restore_command = 'cp /backup/wal_archive/%f %p'
# recovery_target_time = '2024-01-15 12:00:00 UTC'
# recovery_target_action = 'promote'
recovery_target = 'immediate'
recovery_target_action = 'promote'
EOF
```

> 📝 If you do **not** have a WAL archive and want to restore only from the base backup, you can skip the `restore_command`. PostgreSQL will apply only the WAL files bundled inside the backup and then promote.

### 5.4 Update pg_hba.conf

```bash
sudo -u postgres tee -a /var/lib/postgresql/data/pg_hba.conf << 'EOF'

# Replication entries for Patroni cluster
host    replication     replicator      10.0.0.11/32    md5
host    replication     replicator      10.0.0.12/32    md5
host    replication     replicator      10.0.0.13/32    md5
host    all             all             192.168.1.0/24     md5
EOF
```

---

## Step 6: Clean Up DCS State

The old cluster state in your DCS may conflict with the restored cluster. Remove the old Patroni cluster key **before** starting any node.

### For etcd v3

```bash
# List current Patroni keys
etcdctl get /service/your-cluster-name --prefix

# Delete all Patroni cluster keys
etcdctl del /service/your-cluster-name --prefix

# Verify deletion
etcdctl get /service/your-cluster-name --prefix
```

> 📝 Replace `your-cluster-name` with the `scope` value from your `patroni.yml`.

### For etcd v2

```bash
etcdctl rm /service/your-cluster-name --recursive
```

### For Consul

```bash
consul kv delete -recurse service/your-cluster-name/
```

### For ZooKeeper

```bash
zkCli.sh -server localhost:2181 deleteall /service/your-cluster-name
```

> ⚠️ **Important:** Failing to clean DCS state can cause Patroni to refuse to start or trigger a split-brain scenario.

---

## Step 7: Start Patroni on the Primary Node Only

With an empty DCS and a single node running, Patroni will **immediately elect itself as the leader** without waiting for other nodes. This is the correct and expected behavior.

> 💡 **Why this is safe:** Patroni requires a DCS quorum to elect a leader. Since the DCS is empty and only one node is starting, it will acquire the leader lock instantly and promote PostgreSQL to primary with no waiting period.

### 7.1 Start Patroni on node1

```bash
# On node1 only:
sudo systemctl start patroni

# Follow the logs in real time
sudo journalctl -u patroni -f
```

### 7.2 Expected Log Output

Watch for these key messages indicating a successful single-node bootstrap:

```
INFO: Lock owner: None; I am node1
INFO: trying to bootstrap a new cluster
INFO: postmaster pid=12345
INFO: starting as a primary
INFO: promoted self to leader by acquiring session lock
INFO: no action. I am (node1) the leader with the lock
```

> If you see repeated `waiting for leader` messages, verify that you cleared the DCS state in Step 6.

### 7.3 Verify node1 is Running as Primary

```bash
# Check Patroni sees itself as the leader
patronictl -c /etc/patroni/patroni.yml list
```

Expected output with only one node running:

```
+ Cluster: my-postgres-cluster (7890123456789012345) +---------+----+-----------+
| Member | Host              | Role   | State   | TL | Lag in MB |
+--------+-------------------+--------+---------+----+-----------+
| node1  | 10.0.0.11:5432 | Leader | running |  1 |           |
+--------+-------------------+--------+---------+----+-----------+
```

```bash
# Confirm PostgreSQL is not in recovery mode (i.e., it is a primary)
sudo -u postgres psql -c "SELECT pg_is_in_recovery();"

# Expected output:
#  pg_is_in_recovery
# -------------------
#  f
# (1 row)
```

---

## Step 8: Inspect Data with psql

Before bringing up replicas, take time to **validate the restored data**. This is the key advantage of the staged approach — if the data is wrong, you can stop here, fix the issue, and re-restore without affecting any replicas.

### 8.1 Connect to PostgreSQL

```bash
sudo -u postgres psql
# or connect to a specific database
sudo -u postgres psql -d your_database
```

### 8.2 Basic Sanity Checks

```sql
-- List all databases and their sizes
SELECT datname,
       pg_size_pretty(pg_database_size(datname)) AS size
FROM pg_database
ORDER BY pg_database_size(datname) DESC;

-- Connect to your target database
\c your_database

-- List all schemas
\dn

-- List all tables with row estimates
SELECT schemaname,
       tablename,
       pg_size_pretty(pg_total_relation_size(schemaname || '.' || tablename)) AS total_size,
       n_live_tup AS estimated_rows
FROM pg_stat_user_tables
ORDER BY pg_total_relation_size(schemaname || '.' || tablename) DESC;
```

### 8.3 Verify Critical Tables

```sql
-- Check row counts on your most important tables
SELECT COUNT(*) FROM your_schema.your_critical_table;

-- Check the most recent records
SELECT * FROM your_schema.your_critical_table
ORDER BY created_at DESC
LIMIT 10;

-- Check for any tables that appear empty unexpectedly
SELECT schemaname, tablename, n_live_tup
FROM pg_stat_user_tables
WHERE n_live_tup = 0
ORDER BY schemaname, tablename;
```

### 8.4 Verify Database Timeline and LSN

```sql
-- Check current WAL LSN (useful for confirming recovery point)
SELECT pg_current_wal_lsn();

-- Check the timeline history
SELECT timeline_id, reason, written_at
FROM pg_control_checkpoint();

-- Or use pg_controldata from the shell:
-- sudo -u postgres /usr/lib/postgresql/15/bin/pg_controldata /var/lib/postgresql/data/ \
--   | grep -E "TimeLineID|REDO location|state"
```

### 8.5 Check for Corruption

```sql
-- Run a basic check on a specific table (reads all pages)
-- Replace with your actual table name
SELECT COUNT(*) FROM your_schema.your_table;

-- Check for invalid indexes
SELECT indexrelid::regclass AS index_name,
       indisvalid
FROM pg_index
WHERE NOT indisvalid;

-- Check for bloat or corruption hints in pg_stat_user_tables
SELECT relname, last_vacuum, last_autovacuum, last_analyze
FROM pg_stat_user_tables
ORDER BY relname;
```

### 8.6 Decision Point

```
✅ Data looks correct?
   → Proceed to Step 9 to bring up replica nodes.

❌ Data is missing, incorrect, or from the wrong point in time?
   → Stop Patroni: sudo systemctl stop patroni
   → Return to Step 4 and restore a different backup or adjust
     the recovery_target_time in postgresql.conf.
   → Do NOT start replicas until the primary data is confirmed correct.
```

> 💡 **Tip:** Use `patronictl pause` to freeze Patroni's automatic actions while you inspect data, in case Patroni tries to do something unexpected during a long inspection:
>
> ```bash
> patronictl -c /etc/patroni/patroni.yml pause my-postgres-cluster
> # ... inspect data ...
> patronictl -c /etc/patroni/patroni.yml resume my-postgres-cluster
> ```

---

## Step 9: Initialize the Replica Nodes

Once you have confirmed the primary data is correct, prepare the replica nodes.

### Option A: Let Patroni Bootstrap Replicas Automatically (Recommended)

With this approach, replica data directories are empty. When Patroni starts on **node2** and **node3**, it detects no data and automatically runs `pg_basebackup` from the running primary to clone itself.

Ensure the replica data directories are empty (already done in Step 3.2):

```bash
# On node2 and node3 — verify data directory is empty
ls -la /var/lib/postgresql/data/
# Should be empty
```

Ensure `patroni.yml` on replicas is configured for cloning:

```yaml
# /etc/patroni/patroni.yml (on node2 and node3)
postgresql:
  basebackup:
    checkpoint: fast
    max-rate: 100M
    no-password: true
```

Patroni will handle the rest automatically when started in Step 10.

### Option B: Manually Restore Backup on Replicas

Use this option if the network between nodes is slow or you want to pre-seed the replicas from the same backup file to save time.

#### On node2:

```bash
sudo -u postgres bash

# Extract the same base backup
tar -xzf /backup/pg_basebackup/base.tar.gz \
    -C /var/lib/postgresql/data/

# For PostgreSQL >= 12: create standby signal
touch /var/lib/postgresql/data/standby.signal

# Remove patroni.dynamic.json — Patroni will recreate it
rm -f /var/lib/postgresql/data/patroni.dynamic.json

exit

# Fix permissions
sudo chown -R postgres:postgres /var/lib/postgresql/data/
sudo chmod 700 /var/lib/postgresql/data/
```

#### On node3: repeat the same steps as node2.

> 📝 Do **not** manually configure `primary_conninfo` in `postgresql.conf` on replicas — Patroni will write this automatically based on `patroni.yml`.

---

## Step 10: Start Patroni on Replica Nodes

Start replicas **one at a time** so you can monitor each one joining the cluster successfully.

### 10.1 Start Patroni on node2

```bash
# On node2:
sudo systemctl start patroni

# Watch the logs
sudo journalctl -u patroni -f
```

Expected log output on node2 (Option A — automatic clone):

```
INFO: Lock owner: node1; I am node2
INFO: does not have lock
INFO: cloning from leader 'node1'
INFO: basebackup completed
INFO: starting as a standby
INFO: established a streaming replication connection from primary
INFO: no action. I am a secondary and I am following a leader
```

Expected log output on node2 (Option B — pre-seeded):

```
INFO: Lock owner: node1; I am node2
INFO: does not have lock
INFO: starting as a standby
INFO: established a streaming replication connection from primary
INFO: no action. I am a secondary and I am following a leader
```

### 10.2 Verify node2 is Streaming Before Starting node3

```bash
# Check the cluster — node2 should show Replica with 0 lag
patronictl -c /etc/patroni/patroni.yml list
```

```
+ Cluster: my-postgres-cluster (7890123456789012345) +---------+----+-----------+
| Member | Host              | Role    | State   | TL | Lag in MB |
+--------+-------------------+---------+---------+----+-----------+
| node1  | 10.0.0.11:5432 | Leader  | running |  1 |           |
| node2  | 10.0.0.12:5432 | Replica | running |  1 |         0 |
+--------+-------------------+---------+---------+----+-----------+
```

### 10.3 Start Patroni on node3

```bash
# On node3:
sudo systemctl start patroni

# Watch the logs
sudo journalctl -u patroni -f
```

---

## Step 11: Verify the Cluster

### 11.1 Check Full Cluster Status

```bash
patronictl -c /etc/patroni/patroni.yml list
```

Expected output with all nodes running:

```
+ Cluster: my-postgres-cluster (7890123456789012345) +---------+----+-----------+
| Member | Host              | Role    | State   | TL | Lag in MB |
+--------+-------------------+---------+---------+----+-----------+
| node1  | 10.0.0.11:5432 | Leader  | running |  1 |           |
| node2  | 10.0.0.12:5432 | Replica | running |  1 |         0 |
| node3  | 10.0.0.13:5432 | Replica | running |  1 |         0 |
+--------+-------------------+---------+---------+----+-----------+
```

### 11.2 Verify Replication is Working

```bash
# On node1 (primary) — check streaming connections from replicas
sudo -u postgres psql -c "
SELECT client_addr,
       application_name,
       state,
       sync_state,
       sent_lsn,
       write_lsn,
       flush_lsn,
       replay_lsn
FROM pg_stat_replication;"

# On node2/node3 — confirm they are in recovery (replica) mode
sudo -u postgres psql -c "SELECT pg_is_in_recovery();"
# Expected: t

# Check replication lag on replicas
sudo -u postgres psql -c "
SELECT now() - pg_last_xact_replay_timestamp() AS replication_lag;"
```

### 11.3 Test Failover

```bash
# Perform a manual switchover to verify everything works
patronictl -c /etc/patroni/patroni.yml switchover \
    --master node1 \
    --candidate node2 \
    --scheduled now

# Watch cluster status during switchover
watch -n 2 'patronictl -c /etc/patroni/patroni.yml list'

# Switch back to node1
patronictl -c /etc/patroni/patroni.yml switchover \
    --master node2 \
    --candidate node1 \
    --scheduled now
```

### 11.4 Enable Patroni Auto-Start on All Nodes

```bash
# Run on all nodes
sudo systemctl enable patroni
sudo systemctl is-enabled patroni
```

---

## Troubleshooting

### Problem: Patroni on node1 Is Waiting for a Leader Instead of Electing Itself

```bash
# Verify DCS keys are truly empty
etcdctl get /service/your-cluster-name --prefix

# If keys exist, delete them
etcdctl del /service/your-cluster-name --prefix

# Restart Patroni on node1
sudo systemctl restart patroni
sudo journalctl -u patroni -f
```

### Problem: Patroni Refuses to Start — "data directory is not empty"

```bash
# Ensure the data directory is truly empty on replicas
sudo -u postgres bash -c "ls -la /var/lib/postgresql/data/"

# Remove all files including hidden ones
sudo -u postgres bash -c "rm -rf /var/lib/postgresql/data/{*,.[!.]*}"
```

### Problem: node1 Promoted but pg_is_in_recovery() Still Returns True

```bash
# Check if standby.signal still exists
ls -la /var/lib/postgresql/data/*.signal

# Remove it and ask Patroni to reload
sudo -u postgres rm -f /var/lib/postgresql/data/standby.signal
sudo systemctl restart patroni
```

### Problem: Replica Lag Is Not Decreasing

```bash
# Check WAL receiver status on replica
sudo -u postgres psql -c "SELECT * FROM pg_stat_wal_receiver;"

# Check for stale replication slots on primary
sudo -u postgres psql -c "SELECT * FROM pg_replication_slots;"

# Drop a stale slot if present
sudo -u postgres psql -c "SELECT pg_drop_replication_slot('stale_slot_name');"
```

### Problem: "Timeline Mismatch" Error on Replica

```bash
# Check timeline on all nodes
sudo -u postgres /usr/lib/postgresql/15/bin/pg_controldata \
    /var/lib/postgresql/data/ | grep "TimeLineID"

# Force Patroni to reinitialize the replica from scratch
patronictl -c /etc/patroni/patroni.yml reinit \
    your-cluster-name node2 --force
```

### Problem: Replica Cannot Connect to Primary for Streaming

```bash
# Test replication connection manually from replica node
sudo -u postgres psql \
    "host=10.0.0.11 port=5432 user=replicator dbname=replication replication=database" \
    -c "IDENTIFY_SYSTEM;"

# Check pg_hba.conf on primary allows the replica IPs
sudo -u postgres grep replication /var/lib/postgresql/data/pg_hba.conf

# Reload pg_hba.conf on primary without restart
sudo -u postgres psql -c "SELECT pg_reload_conf();"
```

---

## Appendix

### A. Sample Patroni Configuration (patroni.yml)

```yaml
scope: my-postgres-cluster
namespace: /service/
name: node1

restapi:
  listen: 10.0.0.11:8008
  connect_address: 10.0.0.11:8008

etcd3:
  hosts: 127.0.0.1:2379:2379

bootstrap:
  dcs:
    ttl: 30
    loop_wait: 10
    retry_timeout: 10
    maximum_lag_on_failover: 1048576
    postgresql:
      use_pg_rewind: true
      use_slots: true
      parameters:
        wal_level: replica
        hot_standby: "on"
        max_wal_senders: 10
        max_replication_slots: 10
        wal_log_hints: "on"

  initdb:
    - encoding: UTF8
    - data-checksums

  pg_hba:
    - host replication replicator 127.0.0.1/32 md5
    - host replication replicator 192.168.1.0/24 md5
    - host all all 0.0.0.0/0 md5

  users:
    admin:
      password: admin-password
      options:
        - createrole
        - createdb

postgresql:
  listen: 10.0.0.11:5432
  connect_address: 10.0.0.11:5432
  data_dir: /var/lib/postgresql/data
  bin_dir: /usr/lib/postgresql/15/bin
  pgpass: /tmp/pgpass

  authentication:
    replication:
      username: replicator
      password: replicator-password
    superuser:
      username: postgres
      password: postgres-password

  basebackup:
    checkpoint: fast
    max-rate: 100M
    no-password: true

  parameters:
    unix_socket_directories: '/var/run/postgresql'

tags:
  nofailover: false
  noloadbalance: false
  clonefrom: false
  nosync: false
```

### B. Creating a pg_basebackup for Future Use

```bash
sudo -u postgres pg_basebackup \
    --host=10.0.0.11 \
    --username=replicator \
    --pgdata=/backup/pg_basebackup/ \
    --format=tar \
    --gzip \
    --compress=9 \
    --wal-method=stream \
    --checkpoint=fast \
    --label="patroni_full_backup_$(date +%Y%m%d)" \
    --progress \
    --verbose

ls -lh /backup/pg_basebackup/
```

### C. Quick Reference Checklist

```
PRE-RESTORE:
[ ] Stop Patroni on all nodes
[ ] Stop PostgreSQL on all nodes
[ ] Backup patroni.yml on all nodes
[ ] Verify backup integrity and note START WAL LOCATION
[ ] Clear data directories on ALL nodes

RESTORE PRIMARY (node1 only):
[ ] Extract backup on node1
[ ] Set correct permissions (postgres:postgres, 700)
[ ] Remove standby.signal and patroni.dynamic.json
[ ] Configure postgresql.conf (wal_level, max_wal_senders, etc.)
[ ] Configure pg_hba.conf
[ ] Clean DCS state (etcd/consul/zookeeper)

START PRIMARY AND INSPECT:
[ ] Start Patroni on node1 only
[ ] Confirm node1 is Leader in patronictl list
[ ] Confirm pg_is_in_recovery() = f on node1
[ ] Inspect databases, tables, row counts
[ ] Check for index validity and corruption
[ ] Verify data is from the correct point in time
[ ] DECISION: data correct? → proceed. data wrong? → re-restore.

START REPLICAS:
[ ] Start Patroni on node2
[ ] Verify node2 shows Replica with 0 lag in patronictl list
[ ] Start Patroni on node3
[ ] Verify node3 shows Replica with 0 lag in patronictl list

FINAL CHECKS:
[ ] All 3 nodes visible in patronictl list
[ ] pg_stat_replication shows 2 streaming connections on primary
[ ] Replication lag is 0 on both replicas
[ ] Test switchover succeeds
[ ] Enable Patroni auto-start on all nodes (systemctl enable patroni)
```

### D. Useful Commands Reference

```bash
# Patroni cluster management
patronictl -c /etc/patroni/patroni.yml list
patronictl -c /etc/patroni/patroni.yml topology
patronictl -c /etc/patroni/patroni.yml history
patronictl -c /etc/patroni/patroni.yml pause  <cluster>
patronictl -c /etc/patroni/patroni.yml resume <cluster>
patronictl -c /etc/patroni/patroni.yml failover  <cluster>
patronictl -c /etc/patroni/patroni.yml switchover <cluster>
patronictl -c /etc/patroni/patroni.yml reinit <cluster> <node> --force

# PostgreSQL replication checks
psql -c "SELECT * FROM pg_stat_replication;"
psql -c "SELECT * FROM pg_stat_wal_receiver;"
psql -c "SELECT pg_is_in_recovery();"
psql -c "SELECT pg_current_wal_lsn();"           -- primary
psql -c "SELECT pg_last_wal_receive_lsn();"       -- replica
psql -c "SELECT pg_last_wal_replay_lsn();"        -- replica
psql -c "SELECT now() - pg_last_xact_replay_timestamp();" -- replica lag

# etcd cluster health
etcdctl endpoint health --cluster
etcdctl member list
etcdctl get /service/my-postgres-cluster --prefix
etcdctl del /service/my-postgres-cluster --prefix
```

