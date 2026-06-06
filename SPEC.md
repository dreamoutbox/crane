# crane — CLI Deployment Tool: Full Specification

## Overview

`crane` is a Rust CLI tool (using `clap`) that deploys apps and infrastructure to VPS nodes over SSH. No Docker required. Single `crane.toml` config per environment. Manages the full stack: app instances, load balancing (Traefik), databases (Postgres, Redis), object storage (MinIO), DNS (Cloudflare), backups, firewall, and auto-scaling.

---

## Technology Decisions

| Concern | Decision |
|---|---|
| Language | Rust + Clap |
| SSH | Shell out to system `ssh`/`scp` binary. Use `ControlMaster` multiplexing (one connection per deploy, reused across all commands) |
| Config format | TOML |
| Multi-env | Separate files: `dev/crane.toml`, `production/crane.toml`. `--config` flag, defaults to `./crane.toml` |
| OS support | Debian-based first. Abstracted via `ServerInteractor` trait |
| Load balancer | Traefik, file provider, writes to `/etc/traefik/dynamic/{appname}.toml` |
| Deploy strategy | Rolling (per instance: stop → update binary → start → health check → next) |
| Health check | `GET http://127.0.0.1:{port}{health_check_path}` → wait for 200 or timeout |
| Binary delivery | User builds locally, specifies output binary in `crane.toml`, crane `scp`s it |
| Systemd | Template units: `{appname}@.service`, instances as `{appname}@{port}.service` |
| State discovery | Derive from VPS: query `systemctl list-units '{appname}@*.service'` |
| Deploy snapshots | `deploystate/deploy_{datetime}.toml` captures full state for rollback |
| Rollback | Capistrano-style: `/opt/{appname}/releases/{datetime}/`, symlink `/opt/{appname}/current` |
| Monitoring | Client-side terminal poller (Grafana integration deferred to v2) |
| Auto-scaling | SSH reads `/proc/stat` + `/proc/meminfo`, scales within `min_replicas`/`max_replicas` |

---

## Architecture: OS Abstraction

```rust
trait ServerInteractor {
    fn install_packages(&self, pkgs: &[&str]) -> Result<()>;
    fn enable_service(&self, name: &str) -> Result<()>;
    fn start_service(&self, name: &str) -> Result<()>;
    fn stop_service(&self, name: &str) -> Result<()>;
    fn restart_service(&self, name: &str) -> Result<()>;
    fn reload_service(&self, name: &str) -> Result<()>;
    fn service_status(&self, name: &str) -> Result<ServiceStatus>;
    fn list_units(&self, pattern: &str) -> Result<Vec<String>>;
    fn write_file(&self, path: &str, content: &str, mode: u32) -> Result<()>;
    fn firewall_allow(&self, port: u16, proto: &str) -> Result<()>;
    fn firewall_deny(&self, port: u16, proto: &str) -> Result<()>;
}

struct DebianInteractor { ssh: SshSession }
// Future: RhelInteractor, UbuntuInteractor
```

---

## Multi-VPS Node Model

Nodes are declared in `crane.toml` with roles. Crane provisions each node according to its assigned roles. App nodes each run Traefik. DNS round-robins across all app node IPs.

### Roles
- `app` — runs app instances via systemd template units
- `traefik` — runs Traefik load balancer (always paired with `app`)
- `postgres` — runs Postgres (primary on first node, replicas on subsequent)
- `redis` — runs Redis
- `minio` — runs MinIO cluster node

---

## `crane.toml` Full Schema

```toml
# crane.toml

[[nodes]]
host = "localhost"
public_ip = "localhost"
internal_ip = "localhost"
port = 2221
user = "admin"
roles = ["app", "traefik"]

[[nodes]]
host = "localhost"
public_ip = "localhost"
internal_ip = "localhost"
port = 2222
user = "admin"
roles = ["app", "traefik"]

[[nodes]]
host = "localhost"
public_ip = "localhost"
internal_ip = "localhost"
port = 2223
user = "admin"
roles = ["postgres", "redis"]

[[nodes]]
host = "localhost"
public_ip = "localhost"
internal_ip = "localhost"
port = 2224
user = "admin"
roles = ["minio"]

[[users]]
name = "deployman"
groups = ["www-data"]
ssh_authorized_keys = ["~/.ssh/id_rsa.pub"]

[app]
name = "myapp"
binary = "./target/release/myapp"
# Deploy Info
deploy_user = "deployman"
port_start = 3000
instances = 2
dependencies = ["libssl3", "ca-certificates"]
# Health Check
health_check_path = "/health"
health_check_timeout = 30
health_check_interval = 2
# Keep old app versions
retain_releases = 3

[app.env]
APP_ENV = "simulation"
LOG_LEVEL = "debug"

[monitor]
interval = 30

[monitor.autoscale]
min_replicas = 1
max_replicas = 4
scale_up_cpu = 80
scale_down_cpu = 20
scale_up_memory = 85
cooldown = 120

[domain]
name = "myapp.local"
provider = "cloudflare"
tls_email = "dev@example.com"

[db.postgres]
version = "16"
name = "myapp_db"
user = "deployman"
replica_pass = "replica"

[db.redis]
version = "7"
bind = "127.0.0.1"

[backup.s3]
bucket = "crane1"
region = "us-east-1"
endpoint = "http://s3:9000" # omit for AWS S3

access_key_id = "minio"
secret_access_key = "miniominio"

```

---

## `.env` File (gitignored, merged with `[app.env]`)

```env
# App secrets
DATABASE_URL=postgres://dbadmin:secret@localhost/myapp_db
SECRET_KEY=...

# DB passwords
POSTGRES_PASSWORD=...
REDIS_PASSWORD=...

# MinIO
MINIO_ROOT_USER=...
MINIO_ROOT_PASSWORD=...

# S3/Backup
S3_ACCESS_KEY_ID=...
S3_SECRET_ACCESS_KEY=...

# Cloudflare (can also go in crane.toml)
CLOUDFLARE_API_TOKEN=...
```

Crane merges `[app.env]` + `.env`, writes to `/etc/crane/{appname}/.env` on VPS with `600` perms. `.env` takes precedence on conflicts.

---

## CLI Commands

```
crane init                          # scaffold crane.toml + .env.example + .gitignore
crane setup [--config <path>]       # idempotent VPS provisioning (never destructive)
crane deploy [--config <path>] [--no-dns-update]   # rolling deploy
crane status                        # show nodes, instances, ports, health
crane scale <n>                     # manually set instance count
crane rebalance                     # force even traffic rebalance across instances
crane rollback [<deploy-id>]        # restore a previous deploystate snapshot
crane dns sync                      # manual DNS sync (also runs during deploy)
crane logs [--instance <port>] [--since <time>] [--lines <n>]
crane monitor                       # long-running terminal health + auto-scale loop
crane backup run                    # manual backup (all configured services)
crane backup list
crane backup restore <id>
crane db postgres <subcommand>      # db management (create, drop, psql shell, etc.)
crane db redis <subcommand>
```

---

## Deploy Flow (`crane deploy`)

1. Read `crane.toml` + `.env`
2. Open SSH `ControlMaster` connections to all app nodes
3. **DNS check**: resolve `domain.name` → compare to node IPs → update Cloudflare if drift (skip with `--no-dns-update`)
4. For each app node (parallel across nodes, rolling within node):
   a. `scp` binary to `/opt/{appname}/releases/{datetime}/{appname}`
   b. Snapshot full state → `deploystate/deploy_{datetime}.toml`
   c. For each instance port (rolling):
      - `systemctl stop {appname}@{port}`
      - Symlink `/opt/{appname}/current` → new release
      - Write merged env to `/etc/crane/{appname}/.env` (600 perms)
      - `systemctl start {appname}@{port}`
      - Poll `GET http://127.0.0.1:{port}{health_check_path}` until 200 or timeout
      - On timeout: fail deploy, leave other instances running
   d. Regenerate Traefik dynamic config → write to `/etc/traefik/dynamic/{appname}.toml`
   e. `systemctl reload traefik`
5. Prune old releases (keep `retain_releases`)
6. Report final status

---

## Setup Flow (`crane setup`)

Idempotent — checks before installing, never destructive. Run once per environment.

1. Detect OS → instantiate correct `ServerInteractor` (fail fast if unsupported)
2. Create declared users + SSH authorized keys
3. Configure `ufw`: allow 22, 80, 443. App ports (3000+) are localhost-only
4. Per node role:
   - `app` + `traefik`: install Traefik, write static config (ACME/Let's Encrypt, HTTP→HTTPS redirect), write systemd template unit for app, create `/opt/{appname}/releases/` + `/etc/crane/{appname}/`
   - `postgres`: install `postgresql-{version}`, configure primary/replica streaming replication, create DB + role with password
   - `redis`: install `redis`, configure password + `bind 127.0.0.1`, write systemd unit
   - `minio`: install MinIO binary, configure cluster mode with declared `data_dirs`, write systemd unit
5. Enable + start all services

---

## Systemd Template Unit

Crane generates and deploys `/etc/systemd/system/{appname}@.service`:

```ini
[Unit]
Description=crane managed: %p instance on port %i
After=network.target

[Service]
Type=simple
User={deploy_user}
WorkingDirectory=/opt/{appname}
ExecStart=/opt/{appname}/app
EnvironmentFile=/etc/crane/{appname}/.env
Restart=on-failure
RestartSec=5
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true

[Install]
WantedBy=multi-user.target
```

---

## Traefik Dynamic Config (per app, per node)

Written to `/etc/traefik/dynamic/{appname}.toml`, regenerated on each deploy:

```toml
[http.routers.myapp]
  rule = "Host(`myapp.com`)"
  service = "myapp"
  [http.routers.myapp.tls]
    certResolver = "letsencrypt"

[http.services.myapp.loadBalancer]
  [[http.services.myapp.loadBalancer.servers]]
    url = "http://127.0.0.1:3000"
  [[http.services.myapp.loadBalancer.servers]]
    url = "http://127.0.0.1:3001"
```

---

## Rollback

### Release layout on VPS
```
/opt/{appname}/
  releases/
    20240523_143012/myapp
    20240522_091500/myapp
  current -> releases/20240523_143012/myapp   ← symlink
```

### Deploy state snapshot (local)
```
deploystate/
  deploy_20240523_143012.toml   ← binary release path, env snapshot, traefik config, node state
  deploy_20240522_091500.toml
```

`crane rollback` lists snapshots, user picks one (or pass `<deploy-id>`), crane replays: repoints symlink, rewrites env, regenerates Traefik config, restarts all instances.

---

## Postgres Multi-Node Replication

- First node with role `postgres` = **primary**: crane configures `postgresql.conf` (WAL level, max_wal_senders), `pg_hba.conf` (replication slot for each replica)
- Subsequent postgres nodes = **replicas**: crane runs `pg_basebackup`, writes `recovery.conf` / `postgresql.auto.conf` (v12+), starts in standby mode
- Replica promotion on failure: `crane db postgres promote --node <host>` (manual trigger in v1, auto in v2)

---

## Backup Flow

For each configured service:
1. `pg_dump` (Postgres) or `redis-cli SAVE` + copy RDB (Redis)
2. Compress to `.zip`
3. `scp` to local temp
4. Upload to S3/MinIO with prefix from config
5. Prune local copies older than `retain_local` days

```
crane backup run
crane backup list
crane backup restore <id>    # downloads from S3, restores to DB
```

---

## Auto-scaling (Monitor Loop)

`crane monitor` runs as a long-lived client-side process:

1. SSH to each app node every `monitor.interval` seconds
2. Read `/proc/stat` (CPU) and `/proc/meminfo` (memory)
3. Compare against thresholds
4. If scale event needed and outside `cooldown` window:
   - Scale up: `systemctl start {appname}@{next_port}`, add to Traefik config
   - Scale down: remove from Traefik config, `systemctl stop {appname}@{port}`
   - Respect `min_replicas` / `max_replicas`
5. Print status table to terminal (timestamp, node, instances, CPU%, MEM%, last event)

---

## DNS Flow (on `crane deploy`)

1. Resolve `domain.name` via system DNS
2. Compare resolved IPs to crane node IPs (all app nodes)
3. If mismatch: call Cloudflare API, upsert A records for each app node IP (round-robin)
4. Log update; silent if already correct
5. Skip entirely with `--no-dns-update`

Cloudflare token: `CLOUDFLARE_API_TOKEN` from `.env` or `crane.toml`.

---

## File Layout (Local)

```
production/
  crane.toml
  .env                      # gitignored
  .env.example              # committed
deploystate/
  deploy_20240523_143012.toml
  deploy_20240522_091500.toml
.gitignore                  # includes: .env, deploystate/
```

---

## File Layout (VPS)

```
/opt/{appname}/
  releases/
    {datetime}/
      {appname}             # binary
  current -> releases/{datetime}/{appname}

/etc/crane/{appname}/
  .env                      # 600, owned by deploy_user

/etc/systemd/system/
  {appname}@.service        # template unit

/etc/traefik/
  traefik.yml               # static config (ACME, entrypoints)
  dynamic/
    {appname}.toml          # dynamic config (router, lb, TLS)
```

---

## `crane init` Output

```
crane.toml         ← full template with all sections, commented defaults
.env.example       ← all secret keys, empty values
.gitignore         ← adds: .env, deploystate/
```

Prompts for: app name, VPS host(s), domain, then generates.

---

## Edge Cases & Implementation Notes

- **ControlMaster multiplexing**: use `-o ControlMaster=auto -o ControlPath=/tmp/crane-{host} -o ControlPersist=60` on all SSH/SCP calls. Open once, reuse, close after deploy.
- **Failed health check during rolling deploy**: stop rolling, leave healthy instances running, report which instance failed. Do not auto-rollback (user runs `crane rollback`).
- **Traefik hot-reload**: Traefik file provider watches the dynamic config dir — no reload needed. But send `systemctl reload traefik` for static config changes.
- **Port allocation**: scan `ss -tlnp` on VPS to confirm port is free before starting a new instance.
- **ufw before Traefik**: firewall must be configured before Traefik starts, or port 80/443 may be blocked.
- **MinIO cluster quorum**: warn user if fewer than 4 data dirs are specified (erasure coding minimum).
- **Postgres primary election**: first `postgres`-role node in `[[nodes]]` array order is primary. Document this clearly.
- **`crane setup` OS check**: run `lsb_release -is` via SSH, fail fast with clear error if not Debian/Ubuntu.
- **`.env` precedence**: merge order is `[app.env]` (base) → `.env` (override). Clearly document this.
- **Deploy user SSH**: the SSH `user` in `[[nodes]]` is the admin user (for setup). `deploy_user` in `[app]` is the unprivileged runtime user. These are different.
