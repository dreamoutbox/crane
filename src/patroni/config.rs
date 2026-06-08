pub fn build_patroni_config(
    node: &crate::config::NodeConfig,
    pg_version: &String,
    replica_pass: &String,
    pg_nodes: &Vec<crate::config::NodeConfig>,
) -> Result<String, anyhow::Error> {
    let mut etcd_hosts_yaml = String::new();
    for n in pg_nodes {
        etcd_hosts_yaml.push_str(&format!("    - {}:2379\n", n.internal_ip));
    }
    let mut summarize_wal_line = String::new();
    if pg_version.parse::<i32>().unwrap_or(0) >= 17 {
        summarize_wal_line = "summarize_wal: \"on\"".to_string();
    }
    let patroni_yaml = format!(
        r#"scope: postgres-cluster
namespace: /service
name: {node_name}

etcd3:
  hosts:
{etcd_hosts}

restapi:
  listen: 0.0.0.0:8008
  connect_address: {internal_ip}:8008

bootstrap:
  dcs:
    ttl: 20
    loop_wait: 10
    retry_timeout: 10
    maximum_lag_on_failover: 1048576
    postgresql:
      use_pg_rewind: true
      use_slots: true
      parameters:
        listen_addresses: '*'
        wal_level: replica
        max_wal_senders: 20
        max_replication_slots: 20
        hot_standby: "on"
        wal_keep_size: 1024MB
        shared_buffers: 128MB
        wal_log_hints: "on"
        logging_collector: "on"
        log_destination: "stderr"
        log_statement: "mod"
        log_min_duration_statement: -1
        log_line_prefix: '%t [%p]: user=%u db=%d app=%a client=%h '
        archive_mode: "on"
        archive_command: "cp %p /var/lib/postgresql/wal_archive/%f"
        {summarize_wal}
  initdb:
    - encoding: UTF8
    - data-checksums
    
  pg_hba:
    - local   all             postgres                                trust
    - local   all             all                                     trust
    - host    all             all             127.0.0.1/32            trust
    - host    all             all             ::1/128                 trust
    - host    replication     replicator      0.0.0.0/0               scram-sha-256
    - host    all             all             0.0.0.0/0               scram-sha-256

postgresql:
  listen: '*:5432'
  connect_address: {internal_ip}:5432
  data_dir: /var/lib/postgresql/{pg_version}/main
  bin_dir: /usr/lib/postgresql/{pg_version}/bin
  pgpass: /var/lib/postgresql/.pgpass
  authentication:
    replication:
      username: replicator
      password: {replica_pass}
    superuser:
      username: postgres
      password: {replica_pass}
  basebackup:
    - checkpoint: fast
    - no-verify-checksums
    - wal-method: stream
"#,
        node_name = node.name,
        etcd_hosts = etcd_hosts_yaml,
        internal_ip = node.internal_ip,
        summarize_wal = summarize_wal_line,
        pg_version = *pg_version,
        replica_pass = *replica_pass
    );

    Ok(patroni_yaml)
}
