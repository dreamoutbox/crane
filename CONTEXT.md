# Crane Context Glossary

This document defines the core domain concepts and terminology used within the `crane` deployment system.

## Language

**Node**:
A VPS host managed by crane, having specific system roles.
_Avoid_: VPS, Server, Machine

**Postgres Unit**:
The module in crane responsible for database clustering setup, replica replication configuration, and database/role creation.
_Avoid_: DB Manager, Postgres Setup

**Primary Node**:
The single node in a database cluster that is configured to accept read-write connections and acts as the source for streaming replication.
_Avoid_: Master, Leader, Read-Write DB

**Follower Node**:
A standby, read-only database replica node that replicates data from the primary node via streaming replication.
_Avoid_: Slave, Replica, Standby, Read-Only DB

**HAProxy**:
The TCP load balancer deployed on application nodes that routes client database connections to the active primary node.
_Avoid_: DB Gateway, Postgres Proxy

**Backup**:
An archived snapshot of the PostgreSQL database cluster's data and write-ahead logs (WALs).
_Avoid_: Dump, DB Export

**Full Backup**:
A complete, self-contained backup of all files in the database cluster.

**Incremental Backup**:
A backup containing only the blocks/files modified since a previous backup.
_Avoid_: Partial Backup, Delta Backup

**Backup ID**:
A unique timestamp-based identifier assigned to a backup.

**Backup Chain**:
The chronological sequence of backups starting with a Full Backup followed by zero or more Incremental Backups.

**Backup Registry**:
A central manifest tracking all existing backups, their locations, and relationship links.
_Avoid_: Catalog, DB list

