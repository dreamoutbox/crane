# LONG WAIT AT `Polling PostgreSQL cluster health...`

Ran command: `git diff 685a3ef1ff78b3d0f0f574e6e2d928b8aad7c65c cbc06f280c45e396dfca94a68b4622a2ed0e51f2`

Here is the git diff between the two commits and a summary of what fixed the long wait.

### Git Diff Summary
The fix replaced the node-by-node parallel health checks in `assert_postgres_cluster_healthy` with a centralized check (`pg_wait_all_replicas`) queried from the active leader node:

```diff
-    // 4. Assert all patroni instances are healthy
-    assert_postgres_cluster_healthy(pg_nodes).await?;
+    // 4. Wait for all replicas to join the cluster
+    pg_wait_all_replicas(&*leader_interactor, &pg_nodes);
```

---

### What Fixed the Long Wait?

1. **Centralized Querying via Leader (DCS)**: 
   Instead of opening parallel connections and executing commands on each node individually, the fixed version queries `patronictl list` **only on the active leader node** (`leader_interactor`). 
2. **Avoiding Commands on Unhealthy/Initializing Replicas**:
   In the problematic commit, each node checked its local status by executing `curl`, `patronictl`, and `psql` (e.g., `select pg_is_in_recovery()`). Running these commands on replica nodes while they are in the `creating replica` or recovery phase frequently blocked, timed out, or hung. 
3. **State-based Polling**:
   By reading the DCS cluster status directly from the leader, it knows immediately when replica states progress from `creating replica` to `streaming` or `running` without interacting with the replicas directly.