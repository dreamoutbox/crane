# Plan: TODO.md L11-L16

Items: (1) README curl install, (2) `db.postgres.minimal` single-node mode, (3) auto-migration when `minimal` flips.
Order: 1 is independent; 3 depends on 2.

Repo notes: `PostgresConfig` in `src/config.rs`; cluster setup in `src/postgres_unit/setup.rs` (`postgres_setup_wrapper`, Patroni/etcd barriers), called from `src/commands/deploy.rs`; release assets built by `.github/workflows/release.yml` as `crane-<tag>-<target>.tar.gz` (`.zip` for Windows). Integration tests (`tests/postgres/*`) are omitted by request; the user tests manually. Config tests in `tests/config/*`: run one test at a time, e.g. `cargo nextest run --test config --test-threads 1 <test_name> --no-capture`.

### Phase 1: README curl install [STATUS: DONE]
- Goal: One-line CLI install from GitHub releases documented.
- Tasks:
  - [x] Add an install script (e.g. `install.sh` at repo root) that detects OS/arch, maps to release targets (linux gnu/musl x86_64, aarch64, macOS x86_64/arm64), resolves latest tag (or `CRANE_VERSION`), downloads `crane-<tag>-<target>.tar.gz`, extracts, installs to `/usr/local/bin` or `~/.local/bin`.
  - [x] Replace "Compile or download" in README Quick Start step 1 with `curl -fsSL .../install.sh | sh`, plus manual download and version-pin examples; note Windows uses the `.zip`.
  - [x] Run shellcheck on the script.
- Done when: script runs in a clean Docker container and `crane --version` works; README renders the command; no Rust changes, build unaffected.

### Phase 2: `minimal` config option (parse only) [STATUS: DONE]
- Goal: `[db.postgres] minimal` parses with default `false`.
- Tasks:
  - [x] Add `minimal: bool` with `#[serde(default)]` to `PostgresConfig`; add a helper `postgres_is_minimal(config)` next to existing config getters.
  - [x] Validation: minimal requires exactly one node with the `postgres` role (clear error otherwise).
  - [x] Extend `tests/config/parse_config_test.rs` (default false, true, invalid multi-node).
  - [x] Document in README and SPEC.md.
- Done when: config tests pass; existing postgres configs still parse unchanged.

### Phase 3: Deploy single postgres node when minimal [STATUS: DONE]
- Goal: `minimal = true` deploys plain PostgreSQL, with no Patroni, etcd, or replication.
- Tasks:
  - [x] New `src/postgres_unit/setup_minimal.rs` (own function, no change to the cluster path): install postgres, init data dir, configure listen address and `pg_hba`, enable the systemd `postgresql` service.
  - [x] Reuse `setup_postgres_primary` for DBs and users. Check what it assumes about Patroni (the leader lookup via `pg_get_primary`, restarts) and abstract it if needed.
  - [x] Branch in `postgres_setup_wrapper` (or in `deploy.rs`) on the minimal helper.
  - [x] Make dependents mode-aware: HAProxy config (route to the single node directly), app env URIs (`POSTGRES_*_LEADER`/`FOLLOWER` both point to the node), status/logs/promote/demote/reset commands (reject or adapt), and the backup/restore scripts (they currently assume Patroni).
  - [x] Skip etcd setup and the firewall rules for etcd/Patroni ports in minimal mode.
- Done when: `cargo build` has no warnings, config tests pass, and the user manually verifies minimal deploy (Patroni/etcd absent, DB/user exist, backup works).

### Phase 4: Migration between minimal and cluster [STATUS: TODO]
- Goal: Flipping `minimal` auto-migrates data, in both directions.
- Tasks:
  - [ ] Detect the current mode on the server (Patroni service/config present vs plain `postgresql` service); compare with the config.
  - [ ] Minimal to cluster: take a safety backup or `pg_basebackup`, set up etcd and Patroni with the existing node as the initial leader (adopt the existing data dir), then bootstrap the replicas.
  - [ ] Cluster to minimal: promote or pick the leader, stop Patroni, etcd and the replicas, convert the leader's data dir to standalone postgres, and clean up the Patroni/etcd units.
  - [ ] Pre-flight guards: require a healthy source, take a backup first (when S3 is enabled), print the migration plan, and abort without data loss on failure.
- Done when: `cargo build` passes, and the user manually verifies both flip directions (data intact, topology correct) and that re-running deploy with an unchanged config is a no-op.

## Unknowns, risks, assumptions
All confirmed by the user; no open questions.
- [SOLVED] `minimal = true` means one postgres node. Multiple nodes fail validation.
- [SOLVED] false to true = cluster to minimal; true to false = minimal to cluster.
- [SOLVED] Cluster to minimal discards replicas.
- [SOLVED] Integration tests are omitted; manual testing by the user.
- Risk: Phase 3 touches many dependents (HAProxy, backup scripts) and may need splitting into two commits.
- The install script assumes the repository `dreamoutbox/crane` and the current release asset naming.
