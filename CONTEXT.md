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
