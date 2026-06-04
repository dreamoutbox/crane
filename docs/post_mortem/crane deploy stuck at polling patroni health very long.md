# FIX crane deploy stuck at polling patroni health very long

I have successfully fixed the Patroni replica setup issue. You can read the details in the Walkthrough.

## The problem.

when user run `cargo run -- -f demo/crane.toml deploy` the user got stuck at `Polling PostgreSQL cluster health...` very long.

### Post-Edit Summary

- **What Changed:** Moved the `basebackup` configuration under the `postgresql:` section in the Patroni configuration template ([patroni.rs](src/postgres_unit/patroni.rs)), and increased DCS timeouts (`ttl` to `30`, `loop_wait`/`retry_timeout` to `10`).
- **Why:** Patroni ignored `bootstrap.basebackup` during replica cloning. Placing it under `postgresql.basebackup` ensures `pg_basebackup` runs with `--checkpoint=fast` (avoiding waiting for the next checkpoint) and `--wal-method=stream`. Relaxing DCS timeouts resolves warning flags and transient timeouts.



# Walkthrough: Patroni Stuck "Creating Replica" Fix

We resolved the issue where replica nodes got stuck in the `creating replica` state during database provisioning and took 90 seconds (the polling timeout) to complete.

## Changes Made

### postgres_unit

#### [patroni.rs](rc/postgres_unit/patroni.rs)

1. Moved the `basebackup` configuration block from the `bootstrap:` section (where it was commented out) to the `postgresql:` section:
   ```yaml
   postgresql:
     ...
     basebackup:
       - checkpoint: fast
       - no-verify-checksums
       - wal-method: stream
   ```
   *Why:* Patroni ignores `bootstrap.basebackup` settings when performing replica cloning (unless the bootstrap method itself is basebackup, which isn't used here). Placing it under `postgresql.basebackup` ensures Patroni passes `--checkpoint=fast`, `--no-verify-checksums`, and `--wal-method=stream` to `pg_basebackup` when cloning replicas.


2. Increased the dynamic configuration store (DCS) timeouts under `bootstrap.dcs:` to safe, standard defaults:
   - `ttl: 30` (was `5`)
   - `loop_wait: 10` (was `5`)
   - `retry_timeout: 10` (was `5`)

---

## Verification Results

1. **Deploy Duration:**
   - **Before:** The deployment took `117s` because the health check timed out after `90s` while waiting for the replica nodes to transition from `creating replica` to `streaming`.
   - **After:** The health check completed in **`2s`**, and the entire deployment finished in **`29s`**.
2. **Cluster Health status:**
   Running `patronictl list` confirms that all replica nodes are successfully `streaming` with `0` lag:
   ```
   + Cluster: postgres-cluster (7647462242241028576) ------------+-----+------------+-----+
   | Member | Host      | Role    | State     | TL | Receive LSN | Lag | Replay LSN | Lag |
   +--------+-----------+---------+-----------+----+-------------+-----+------------+-----+
   | vps1   | 10.0.0.11 | Leader  | running   |  1 |             |     |            |     |
   | vps2   | 10.0.0.12 | Replica | streaming |  1 |   0/4000000 |   0 |  0/4000000 |   0 |
   | vps3   | 10.0.0.13 | Replica | streaming |  1 |   0/4000000 |   0 |  0/4000000 |   0 |
   +--------+-----------+---------+-----------+----+-------------+-----+------------+-----+
   ```

