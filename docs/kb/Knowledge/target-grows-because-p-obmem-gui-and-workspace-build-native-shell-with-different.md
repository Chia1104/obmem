---
type: fact
title: target grows because -p obmem-gui and --workspace build Native Shell with different features
status: draft
author: agent
confidence: high
created: 2026-11-09
updated: 2026-11-09
verified_commit: 66ff8bd
code_refs:
- Cargo.toml
- apps/obmem-gui/tests/gui/common.rs#sandbox
tags:
- build
- cargo
- gui
---
On 2026-11-09 `target/` was 29G, plus 3.9G in `apps/obmem-gui/target`. `target/debug/deps` held 7 copies of `libnsh_base` (347M each) and 8 copies of `nsh_platform_macos`. Every copy had the same rustc, features, profile and rustflags; they differed only in the hashes of their dependencies.

Cause: feature unification depends on which packages a cargo command selects. `rmcp` (obmem-mcp) turns on `serde/rc`, `serde_json/alloc`, `chrono/serde`, tokio `rt-multi-thread` and other features. `obmem` turns on the `clap` feature of obmem-core and obmem-agents. So `cargo test -p obmem-gui` and `cargo test --workspace` resolve serde differently, and everything above serde, Native Shell included, is rebuilt under a new hash. Cargo never deletes the old copies.

- `resolver.feature-unification = "workspace"` is still unstable in cargo 1.99. Without `-Zfeature-unification` it prints "ignoring".
- The `nsh` CLI builds in `apps/obmem-gui/target`, separate from the workspace `target/`.

Applied the same day:
- `[profile.dev.package."*"] debug = false` in the root Cargo.toml. nsh_base dropped from 347M to 144M.
- GUI integration tests merged into one binary, `tests/gui/main.rs`. Each test still gets its own process: `common::sandbox` reruns it in a child process, which it finds by the test thread's name.
- `apps/obmem-gui/AGENTS.md` now says to prefer `--workspace`.

After `cargo clean`, a full `cargo test --workspace` plus clippy left `target/` at 3.3G.

Not done: `cargo-hakari` (a workspace-hack crate) would fix the cause but is one more crate to maintain.
