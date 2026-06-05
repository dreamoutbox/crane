# crane — CLI Deployment Tool

`crane` is a lightweight, zero-Docker deployment tool written in Rust that provisions and deploys application services and high-availability database clusters directly to Ubuntu/Debian VPS nodes over SSH.

---

## Features

- **Zero-Docker App Deploys**: Runs your binary directly on remote nodes as systemd template services.
- **High-Availability PostgreSQL**: Automated cluster topology with Patroni, etcd as DCS, and HAProxy for smart write/read routing.
- **Built-in Reverse Proxy & SSL**: Automatically configures load-balancing and SSL certificates.
- **Automated S3 Backups**: Integrated scheduling for database backups (full/incremental) to S3/MinIO.
- **Cloudflare DNS Sync**: Syncs public node IPs to Cloudflare A records automatically.
- **Security-First Firewall**: Configures UFW for internal cluster ports while exposing public HTTP (80), HTTPS (443), and SSH (22).

---

## Quick Start

1. **Install Crane**:
   Compile or download the `crane` binary and place it in your `$PATH`.

2. **Scaffold Configuration**:
   Create a `crane.toml` configuration and `.env` secrets file in your repository:

   ```toml
   # crane.toml

   [[nodes]]
   name = "vps1"
   host = "localhost"
   public_ip = "10.0.0.11"
   internal_ip = "10.0.0.11"
   port = 2221
   roles = ["app", "haproxy", "postgres", "redis"]
   user = "crane"
   private_key = "keys/id_ed25519"
   sudo_pass = "${SUDO_PASS_VPS1}"

   [[nodes]]
   name = "vps2"
   host = "localhost"
   public_ip = "10.0.0.12"
   internal_ip = "10.0.0.12"
   port = 2222
   roles = ["app", "haproxy", "postgres", "redis"]
   user = "crane"
   private_key = "keys/id_ed25519"
   sudo_pass = "${SUDO_PASS_VPS2}"

   [[nodes]]
   name = "vps3"
   host = "localhost"
   public_ip = "10.0.0.13"
   internal_ip = "10.0.0.13"
   port = 2223
   roles = ["app", "haproxy", "postgres", "redis"]
   user = "crane"
   private_key = "keys/id_ed25519"
   sudo_pass = "${SUDO_PASS_VPS3}"


   [[users]]
   name = "deployman"
   groups = ["www"]
   ssh_authorized_keys = ["keys/id_ed25519.pub"]
   private_key = "keys/id_ed25519"


   [app.myapp]
   name = "myapp"
   # Setup Info
   dependencies = ["libssl3", "ca-certificates"]
   # Deploy Info
   deploy_dir = "./demo"
   entrypoint = "./myapp"
   pre_deploy_script = "./before-deploy.sh"
   deploy_user = "deployman"
   port_start = 3000
   port_end = 3100
   # Health Check
   health_check_path = "/health"
   health_check_timeout = 30
   health_check_interval = 2
   # Domain
   domain = "myapp.localhost"
   # Replicas
   instances = 2
   min_replicas = 1
   max_replicas = 3
   # ENV for myapp
   [app.myapp.env]
   APP_ENV = "simulation"
   LOG_LEVEL = "debug"
   APP_NAME = "myapp"
   # Databases for myapp
   [[app.myapp.database]]
   databases = "mydb"
   user = "u1"

   [app.myapp2]
   name = "myapp2"
   # Setup Info
   dependencies = ["libssl3", "ca-certificates"]
   # Deploy Info
   deploy_dir = "./demo"
   entrypoint = "./myapp"
   deploy_user = "deployman"
   port_start = 4000
   port_end = 4100
   # Health Check
   health_check_path = "/health"
   health_check_timeout = 30
   health_check_interval = 2
   # Domain
   domain = "myapp2.localhost"
   # Replicas
   instances = 1
   min_replicas = 1
   max_replicas = 3
   # ENV for myapp2
   [app.myapp2.env]
   APP_ENV = "simulation"
   LOG_LEVEL = "debug"
   APP_NAME = "myapp2"
   # Databases for my app2
   [[app.myapp2.database]]
   databases = "mydb"
   user = "u1"


   [db.postgres]
   enabled = true
   version = "17"
   replica_pass = "replica"

   [db.postgres.backup]
   full_backup_every = "1h"
   incremental_backup_every = "15m"

   [db.postgres.mydb]
   name = "mydb"

   [[db.postgres.users]]
   state = "present"
   user = "u1"
   password = "u1"
   databases = ["mydb"]


   [backup.s3]
   bucket = "crane1"
   region = "us-east-1"
   endpoint = "http://s3:9000"
   access_key_id = "${S3_ACCESS_KEY_ID}"
   secret_access_key = "${S3_SECRET_ACCESS_KEY}"


   [domain]
   provider = "cloudflare"
   domain_name = "localhost"
   token = "${CLOUDFLARE_TOKEN}"
   ```

3. **Deploy**:
   Make sure environment variables referenced in `crane.toml` (such as `S3_ACCESS_KEY_ID`, `S3_SECRET_ACCESS_KEY`, and `CLOUDFLARE_TOKEN`) are set in your current shell or defined in your `.env` file before deploying:
   ```bash
   crane deploy
   ```

---

## How It Works

1. **Multiplexed SSH**: Crane opens a single multiplexed master SSH connection (`ControlMaster`) to each node.
2. **Security Provisioning**: Resets and configures the host firewall (UFW) to only expose ports 22, 80, and 443 publicly, and fully whitelist inter-node communication.
3. **Database Topology**: Configures Patroni and etcd clusters on nodes with the `postgres` role.
4. **App Delivery**: Compresses the application, transfers it over SCP, extracts it, merges environmental variables, starts the systemd service instances, and polls health checks.
5. **Reverse Proxying**: Hooks up HAProxy/Traefik reverse proxies to distribute incoming public traffic across the running instances.

---

## Maintenance Commands

| Command | Description |
| :--- | :--- |
| `crane status` | Display cluster status, node health, and app instances. |
| `crane pg status` | Inspect the PostgreSQL active primary/replica topologies and replication lag. |
| `crane pg promote --node <host>` | Force-promote a specific node to primary database leader. |
| `crane pg backup <full\|incr>` | Trigger a manual full or incremental cluster database backup to S3. |
| `crane pg list` | List available database backups. |
| `crane pg restore <backup_id> [--pitr <TIME>]` | Restore the database state from an S3 backup ID. with  Point-in-time recovery support. --pitr \<YYYY-MM-DD HH:MM:SS\> |
| `crane logs <app>` | Stream application standard output/error logs. |

---

## Developer Setup

### Prerequisites

- Rust (latest stable edition)
- Docker and Docker Compose (to run the simulated VPS setup)

### Setting Up the Dev Environment

1. **Start the Simulated VPS Cluster**:
   ```bash
   # Install required packages for dev
   ./devsetup.sh

   # Setup dev SSH keys
   ./setup-ssh.sh

   # Setup docker for simulate VPS setup
   ./setup-docker.sh
   ```
2. **Run Code Verification**:
   Ensure you use `cargo nextest` to execute tests. Run single test suites at a time to prevent performance bottlenecks:

   ```bash
   cargo nextest run --test deploy -- test_single_deploy --nocapture
   ```

---

## Contributing

Contributions are highly welcome! Feel free to open issues or submit pull requests. For large architectural changes, please open an issue first to discuss the design.
