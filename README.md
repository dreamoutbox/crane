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
   host = "192.168.1.100"
   public_ip = "192.168.1.100"
   internal_ip = "192.168.1.100"
   port = 22
   user = "deploy"
   roles = ["app", "postgres"]

   [app.myapp]
   name = "myapp"
   deploy_dir = "./dist"
   entrypoint = "./myapp"
   deploy_user = "www-data"
   port_start = 3000
   instances = 2
   dependencies = ["libssl3", "ca-certificates"]
   database = [{ databases = "mydb", user = "u1" }]

   [db.postgres]
   enabled = true
   version = "17"
   replica_pass = "replica_secret_pass"

   [db.postgres.mydb]
   name = "mydb"

   [[db.postgres.users]]
   user = "u1"
   password = "${DB_PASSWORD}"
   databases = ["mydb"]

   [backup.s3]
   bucket = "myapp-backups"
   region = "us-east-1"
   endpoint = "https://s3.amazonaws.com"
   access_key_id = "${S3_ACCESS_KEY_ID}"
   secret_access_key = "${S3_SECRET_ACCESS_KEY}"

   [domain]
   provider = "cloudflare"
   domain_name = "myapp.example.com"
   token = "${CLOUDFLARE_TOKEN}"

   [monitor]
   interval = 30

   [monitor.autoscale]
   min_replicas = 1
   max_replicas = 4
   scale_up_cpu = 80
   scale_down_cpu = 20
   scale_up_memory = 85
   cooldown = 120
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
| `crane pg restore <backup_id> [--pitr <TIME>]` | Restore the database state from an S3 backup ID. with  Point-in-time recovery support. --pitr <YYYY-MM-DD HH:MM:SS> |
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
