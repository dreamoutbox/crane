# Crane Port Reference

This document maps all public and private/internal port numbers used by `crane` and its managed services.

## Public Ports (Accessible from Internet)

These ports must be allowed from any source globally to ensure public client access and administration.

| Port | Protocol | Service | Description |
| :--- | :--- | :--- | :--- |
| **22** | TCP | SSH | Used for deployment, server management, and admin access. |
| **80** | TCP | HTTP | Inbound web traffic, managed by HAProxy. Routes traffic to app instances and handles HTTP-to-HTTPS redirection. |
| **443** | TCP | HTTPS | Inbound secure web traffic, managed by HAProxy. Terminates TLS using Let's Encrypt certificates. |

---

## Private / Internal Ports (Intra-Cluster Only)

These ports are used for communication between nodes in the cluster or only on `localhost`. They must be blocked from the public internet.

| Port | Protocol | Service | Scope | Description |
| :--- | :--- | :--- | :--- | :--- |
| **2379** | TCP | etcd Client API | Cluster Internal | Used by Patroni nodes to talk to the etcd Distributed Consensus Store (DCS). |
| **2380** | TCP | etcd Peer | Cluster Internal | Used for synchronization and consensus election between etcd cluster members. |
| **5000** | TCP | HAProxy Primary | Cluster Internal | Writable PostgreSQL connection pool managed by HAProxy pointing to the active Patroni leader. |
| **5001** | TCP | HAProxy Replica | Cluster Internal | Read-only PostgreSQL connection pool managed by HAProxy pointing to standby followers. |
| **5432** | TCP | PostgreSQL | Localhost / Cluster | Underlying PostgreSQL database instance port, managed by Patroni. |
| **8008** | TCP | Patroni REST API | Cluster Internal | Used for Patroni cluster health checks, failover coordination, and HAProxy leader election health queries. |
| **3000+** | TCP | App Instances | Localhost Only | Ports dynamically assigned to your app instances (e.g. 3000, 3001, etc.). Exposed only on `127.0.0.1` and reverse-proxied locally by HAProxy. |
