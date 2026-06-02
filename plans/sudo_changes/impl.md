# Implementation Plan - Sudo Password Support

Provide support for nodes requiring explicit sudo passwords when executing administrative tasks.



### Agreed Design

We've aligned on all the design decisions. Here's the summary:


| Decision | Answer |
|---|---|
| **Who owns sudo_pass?** | `DebianInteractor` (it builds sudo commands) |
| **How is it passed?** | `DebianInteractor::new(ssh, sudo_pass: Option<String>)` |
| **Injection pattern** | Helper method `sudo_prefix()` → returns `echo '<pass>' \| sudo -S` or just `sudo` |
| **Config struct** | Add `pub sudo_pass: Option<String>` to `NodeConfig` |
| **Scope** | Update `get_server_interactor` signature + all ~8 call sites in one pass |

### Changes needed:
1. **`config.rs`** — add `sudo_pass: Option<String>` to `NodeConfig`
2. **`server_interactor/debian.rs`** — add `sudo_pass` field, `sudo_prefix()` helper, replace all `"sudo"` literals
3. **`server_interactor/mod.rs`** — thread `sudo_pass` through `get_server_interactor` + `get_interactor_for_distro`
4. **All call sites** (deploy.rs, status.rs, logs.rs, haproxy.rs, postgres setup.rs, postgres helper.rs) — pass `node.sudo_pass.clone()` to `get_server_interactor`

Want me to proceed with the implementation?

## Proposed Changes

### Configuration Layer

#### [MODIFY] [config.rs](./src/config.rs)
- Add `pub sudo_pass: Option<String>` to `NodeConfig` to allow TOML parsing of the `sudo_pass` field.

### Server Interactor Layer

#### [MODIFY] [debian.rs](./src/server_interactor/debian.rs)
- Add `sudo_pass: Option<String>` field to `DebianInteractor`.
- Update `DebianInteractor::new` to accept `sudo_pass: Option<String>`.
- Add a helper method `wrap_sudo(&self, cmd: &str) -> String` to wrap commands containing `"sudo"` with `echo '<password>' | sudo -S sh -c "<escaped_cmd>"` when a password is provided.
- Update `run_stdout` and all direct calls to `self.ssh.run_cmd(...)` in `debian.rs` to wrap commands with `wrap_sudo`.

#### [MODIFY] [mod.rs](./src/server_interactor/mod.rs)
- Update `get_server_interactor` and `get_interactor_for_distro` to accept `sudo_pass: Option<String>` and pass it to `DebianInteractor::new`.

### Call Sites (Plumbing `sudo_pass`)

#### [MODIFY] [deploy.rs](./src/commands/deploy.rs)
- Pass `node.sudo_pass.clone()` to `get_server_interactor` at all call sites.

#### [MODIFY] [status.rs](./src/commands/status.rs)
- Pass `node_clone.sudo_pass.clone()` to `get_server_interactor` at the status query call site.

#### [MODIFY] [logs.rs](./src/commands/logs.rs)
- Pass `target.node.sudo_pass.clone()` to `get_server_interactor` when querying logs.
- Update `build_cmd` and `ssh.spawn_cmd` inside follow-mode to wrap the journalctl command if a sudo password is present on the node.

#### [MODIFY] [setup.rs](./src/postgres_unit/setup.rs)
- Pass `node.sudo_pass.clone()` to `get_server_interactor` when configuring nodes.

#### [MODIFY] [helper.rs](./src/postgres_unit/helper.rs)
- Pass `node.sudo_pass.clone()` to `get_server_interactor` inside `connect_to_node`.

#### [MODIFY] [haproxy.rs](./src/haproxy_unit/haproxy.rs)
- Pass `app_node.sudo_pass.clone()` to `get_server_interactor` inside `setup_haproxy_on_each_nodes_wrapper`.

## Verification Plan

### Automated Tests
- Run `cargo nextest run test_deploy -- --no-capture` to ensure deploy
