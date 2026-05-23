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

