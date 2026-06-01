---
trigger: always_on
---

## Project Context
- Language: Rust
- Spec: `./SPEC.md` — read before making design decisions

## Commands
- Run: `cargo run --bin crane -- -f demo/crane.toml [OPTIONS] <COMMAND>`
- Test: `cargo nextest run` — read `tests/README.md` before writing or running tests

## Infrastructure
- VPS environment is simulated via Docker

## Code Style
- Comments: concise, only where non-obvious

## Debugging
Capture and inspect command output via `dbg!`:

```rust
let output = interactor.cmd("whoami")?;
dbg!(&output);
```