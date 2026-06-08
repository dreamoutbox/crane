# Ubuntu vs Rocky Linux

Here are the key package, command, and path differences between **Ubuntu Server** and **Rocky Linux / RHEL 9** that relate directly to the services and setup managed by `crane`:

### 1. Services & System Commands
| Task / Component | Ubuntu / Debian | Rocky Linux / RHEL |
|---|---|---|
| **SSH service name** | `ssh` (`systemctl enable ssh`) | `sshd` (`systemctl enable sshd`) |
| **Admin sudo group** | `sudo` (`usermod -aG sudo crane`) | `wheel` (`usermod -aG wheel crane`) |
| **Systemd PID 1 Path** | `/lib/systemd/systemd` | `/usr/lib/systemd/systemd` |

### 2. Package Manager & Essential Packages
| Package | Ubuntu (`apt-get`) | Rocky Linux (`dnf`) |
|---|---|---|
| **EPEL repository** | *(Not applicable)* | `epel-release` *(needed for patroni, ufw/firewalld)* |
| **Unzip tool** | `unzip` | `unzip` |
| **Process monitoring** | `procps` | `procps-ng` |
| **IP routing utilities** | `iproute2` | `iproute` |
| **Vim editor** | `vim` | `vim-minimal` / `vim` |

### 3. Firewall (UFW vs. Firewalld)
* **Ubuntu (UFW)**: 
  - Package: `ufw`
  - Command example: `ufw allow 80/tcp`
* **Rocky Linux (Firewalld)**:
  - Package: `firewalld`
  - Command example: `firewall-cmd --permanent --add-port=80/tcp` followed by `firewall-cmd --reload`

### 4. PostgreSQL 17 (Official Repositories & Layout)
* **Packages**:
  - **Ubuntu**: `postgresql-17`, `postgresql-client-17`
  - **Rocky**: `postgresql17-server`, `postgresql17`
* **Binary Directory**:
  - **Ubuntu**: `/usr/lib/postgresql/17/bin/`
  - **Rocky**: `/usr/pgsql-17/bin/` (e.g. `/usr/pgsql-17/bin/pg_ctl`)
* **Default Data Directory**:
  - **Ubuntu**: `/var/lib/postgresql/17/main`
  - **Rocky**: `/var/lib/pgsql/17/data`

### 5. clustering / HA Components
* **etcd**:
  - **Ubuntu**: Separate packages `etcd-server` and `etcd-client`
  - **Rocky (EPEL)**: Single package `etcd` containing both server and `etcdctl` client
* **Patroni**:
  - **Ubuntu**: `patroni`
  - **Rocky (EPEL)**: `patroni` (often depends on `python3-patroni` package)