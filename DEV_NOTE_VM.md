# Tes CURL
 
curl -L -k -i \
 --resolve myapp.localhost:80:192.168.1.100 \
 --resolve myapp.localhost:443:192.168.1.100 \
 --resolve myapp2.localhost:80:192.168.1.100 \
 --resolve myapp2.localhost:443:192.168.1.100 http://myapp.localhost/pg

## Etcd listening problem
```
creating peer listener failed: listen tcp 192.168.1.100:2380: bind: cannot assign requested address 
listen tcp 192.168.1.100:2380: bind: cannot assign requested address
```

### get etcd log command 
```sh
sudo journalctl --no-pager -n50 -u etcd
```

### etcd config
```
# Member settings
ETCD_NAME="vps1"
ETCD_DATA_DIR="/var/lib/etcd/default.etcd"
ETCD_LISTEN_PEER_URLS="http://0.0.0.0:2380"
ETCD_LISTEN_CLIENT_URLS="http://0.0.0.0:2379"

# Clustering settings
ETCD_INITIAL_ADVERTISE_PEER_URLS="http://192.168.1.100:2380"
ETCD_INITIAL_CLUSTER="vps1=http://192.168.1.100:2380"
ETCD_INITIAL_CLUSTER_STATE="new"
ETCD_INITIAL_CLUSTER_TOKEN="etcd-postgres-token"
ETCD_ADVERTISE_CLIENT_URLS="http://192.168.1.100:2379"

```

### patroni list command
```sh
sudo patronictl -c /etc/patroni/config.yml list
```

# FIX patroni rest api /primary return 200 but psql `SELECT pg_is_in_recovery();` return true

## TLDR: FIXED use `crane pg reset --force`

In a healthy single-server deployment, the single database node is elected as the Patroni leader and becomes the active writable primary. Because it is the primary database (and not a standby/replica), it is **not** in recovery, so `select pg_is_in_recovery();` returns `f` (false). The function then returns `Ok(Some(node))` because that single node is indeed the primary/leader node.

However, in your deployment run, the deployment timed out because **PostgreSQL got stuck in recovery** during startup, returning `t` (true) instead of `f`.

### Root Cause of the Timeout
From checking the PostgreSQL logs (`/var/lib/postgresql/17/main/log/postgresql-*.csv`) and Patroni status on `vps1`:
1. **Patroni** believes it is the leader (which is why `curl http://127.0.0.1:8008/primary` returned `200`):
   ```
   vps1 | 192.168.1.100 | Leader | running | 14 |
   ```
2. **PostgreSQL 17** is stuck starting up because the new WAL summarization feature (`summarize_wal: "on"`) is waiting for a WAL segment that has already been deleted:
   ```
   "still waiting for WAL summarization through 0/6000308 after 40 seconds"
   "requested WAL segment pg_wal/000000010000000000000002 has already been removed"
   ```
3. Because PostgreSQL is stuck in the startup/recovery phase waiting for WAL summarization, `pg_is_in_recovery()` returns `t`, causing `postgres_get_primary` to return `Ok(None)` and time out.

### How to Fix
To resolve the startup loop, you can clean up the stale PostgreSQL data directory from the previous run on `vps1` (since the DCS state was cleared, starting with a fresh data directory is safest):
```sh
# Connect to vps1 and remove the old data directory to let Patroni bootstrap a fresh one
ssh -i keys/id_ed25519 -p 2222 u@192.168.1.100
sudo systemctl stop patroni
sudo rm -rf /var/lib/postgresql/17/main
sudo systemctl start patroni
```
