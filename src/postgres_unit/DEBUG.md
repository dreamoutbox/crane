## DEBUG

### 1. Replication Status on Primary (`vps1`)

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

### 2. Standby/Recovery Status on Follower (`vps2`)

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

### 3. HAProxy Load Balancer

```sh
docker exec vps1 pg_isready -h 127.0.0.1 -p 5000
```

expected output:
```
127.0.0.1:5000 - accepting connections
```

### Get postgres status

```sh
docker exec vps1 sudo -u postgres psql -t -A -c "show server_version;"
```
