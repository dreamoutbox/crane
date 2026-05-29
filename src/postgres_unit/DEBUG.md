## DEBUG

### 1. check Replication Status on Primary (`vps1`)

```sh
docker exec vps1 sudo -u postgres psql -c "select client_addr, state, sync_state from pg_stat_replication;"
```

expected output:
```
 client_addr |   state   | sync_state 
-------------+-----------+------------
 10.0.0.12   | streaming | async
(1 row)
```

### 2. check Standby/Recovery Status on Follower (`vps2`)

```sh
docker exec vps2 sudo -u postgres psql -c "select pg_is_in_recovery();"
```

expected output:
```
 pg_is_in_recovery 
-------------------
 t
(1 row)
```

### 3. HAProxy Load Balancer check command

```sh
docker exec vps1 pg_isready -h 127.0.0.1 -p 5000
```

expected output:
```
127.0.0.1:5000 - accepting connections
```

### Get postgres status command

```sh
docker exec vps1 sudo -u postgres psql -t -A -c "show server_version;"
```

## FILES

### Postgres Logs at

```
/var/lib/postgresql/17/main/log
```

### Config at

```
/var/lib/postgresql/17/main/postgresql.conf
```

### Restore PITR test command



# Patroni & Etcd Debug Commands

cargo run -- -f demo/crane.toml postgres restore --pitr "2026-05-26 03:26:00" 20260526032538830

### check etcd logs journalctl
```sh
docker exec vps1 journalctl -u etcd -n 50
```

### check patroni logs journalctl
```sh
docker exec vps1 journalctl -xeu patroni.service -n 50
```
