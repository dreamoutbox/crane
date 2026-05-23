## DEBUG

### 1. Replication Status on Primary (`vps1`)

```bash
docker exec vps1 sudo -u postgres psql -c "select client_addr, state, sync_state from pg_stat_replication;"
```

expected output:
```
 client_addr |   state   | sync_state 
-------------+-----------+------------
 10.0.0.12   | streaming | async
(1 row)
```

### 2. Standby/Recovery Status on Follower (`vps2`)

```bash
docker exec vps2 sudo -u postgres psql -c "select pg_is_in_recovery();"
```

expected output:
```
 pg_is_in_recovery 
-------------------
 t
(1 row)
```

### 3. HAProxy Load Balancer

```bash
docker exec vps1 pg_isready -h 127.0.0.1 -p 5000
```

expected output:
```
127.0.0.1:5000 - accepting connections
```


## Promote/Demote node

- Promote `vps2` to leader:
```bash
cargo run -- postgres promote vps2
```

- Verify status:
  - `vps2` should return `pg_is_in_recovery() -> f`
  - `vps1` should return `pg_is_in_recovery() -> t`
  - HAProxy should route connections to `vps2`.

- Demote `vps2` to follower (after making `vps1` leader again):
```bash
cargo run -- postgres promote vps1
```
