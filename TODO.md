# TODO

- [x] change `user` in TOML config `[[nodes]]` to `ssh_user`

- [x] add `enabled` option in TOML config `[domain]`, if disabled ignore domain in `[app.myapp]`.

- [x] add `firewall_expose_port_range` option in TOML config `[[nodes]]`. to allow incoming connections with IP. incase no domain setup, 

- [x] add `enabled` option in TOML config `[backup.s3]`. if disabled ignore backup.

- [x] add curl install command instruction in README to download and install CLI from github releases.

- [x] add `minimal` in `[db.postgres]`. if true, don't deploy postgres cluster, setup replication, no setup Patroni/etcd. just deploy single postgres node.

- [x] deploy from minimal to postgres cluster if `db.postgres.minimal` change from `false` to `true`, and vice versa. auto run data migration.
