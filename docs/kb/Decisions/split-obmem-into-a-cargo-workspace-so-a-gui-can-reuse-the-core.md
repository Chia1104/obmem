---
type: decision
title: Split obmem into a Cargo workspace so a GUI can reuse the core
status: active
author: agent
confidence: high
created: 2026-11-09
updated: 2026-11-10
verified_commit: 38a6b4e
code_refs:
- Cargo.toml
- crates/obmem-core/src/lib.rs
- crates/obmem-core/src/audit.rs#report_text
- crates/obmem-agents/src/lib.rs
- crates/obmem-mcp/src/lib.rs#serve
tags:
- architecture
- workspace
- gui
- nsh
approved_at: 2026-11-10
approved_by: auto-approve
---
**Context.** obmem was a single binary crate (16 modules, ~12.7k lines). The user plans a GUI settings and management app built with Native Shell (NSH, github.com/longbridge/nsh, nsh.dev). NSH Rust apps are ordinary Cargo crates with an `nsh.json` next to them, and they depend on `nsh-core` / `nsh-base` from crates.io. So the GUI needs obmem's logic as a library.

**Decision (2026-11-09, commit 5662297).** A virtual workspace with shared `[workspace.package]` and `[workspace.dependencies]`:
- `obmem-core`: the memory model (its modules are listed in `crates/obmem-core/src/lib.rs`). `audit` and `store` import each other, so they must stay in one crate.
- `obmem-mcp`: the MCP server. It is the only crate that depends on `rmcp`.
- `obmem-agents`: hooks and setup. The GUI's management screens call `setup`.
- `obmem`: the clap binary. The package and binary names are unchanged (`cargo install --path crates/obmem`).
- `clap::ValueEnum` derives are behind an optional `clap` feature in core and agents. `schemars` stays a regular dependency, because the MCP schemas derive from core's enums.

**Since then.**
- The GUI joined as a workspace member: `members = ["crates/*", "apps/*"]` with `default-members = ["crates/*"]`, so a plain `cargo build` stays free of NSH. Its app id is `io.github.chia1104.obmem`.
- Two more library crates follow the same rule of keeping heavy dependencies out of core: `obmem-sync` ([[personal-vault-backup-and-sync-are-built-into-obmem-over-user-provided-s3-r2]]) and `obmem-update` ([[obmem-update-replaces-only-standalone-installs-by-running-the-release-s-install-]]).
- obmem-core's blocking calls run off the UI thread through `nsh_core::blocking` (see `apps/obmem-gui/AGENTS.md`).

**Alternatives.** Only core plus bin: this would put rmcp/tokio and hook code into the GUI's dependencies. One crate per module: boilerplate, and the audit/store cycle rules it out anyway.

**NSH GUI: verified on 2026-11-09 with nsh 0.4.0, in a scratch copy of the repo.**
- `nsh init apps/obmem-gui --template rust` writes a standalone crate with its own empty `[workspace]`. Both layouts work: standalone, or a member of the obmem workspace (remove the app's `[workspace]`, add `"apps/*"` to `members`, set `default-members = ["crates/*"]`). In both, `nsh build` and `nsh package --target macos` succeed, and a headless `nsh_testing::Headless` test in `cargo test` shows text computed with `obmem_core::config::Config::load`.
- This works because nsh writes its runners into `<app>/.nsh/rust/`, which is a Cargo workspace of its own with its own `Cargo.lock`, and it pulls the app crate in by absolute path. The outer workspace does not interfere.
- In member mode the root `Cargo.lock` grew from 142 to 592 packages. obmem's existing dependency versions did not change.
- **Toolchain:** nsh-core 0.4.0 needs the latest stable Rust (it uses `if let` guards and `isolate_lowest_one`). It failed on 1.91.1 and builds on 1.99.1. NSH's repo pins `channel = "stable"`, not an MSRV. `huacnlee/cargo` is *not* required: it is only a cargo fork that shares the build dir across worktrees.
- The nsh CLI builds runners against NSH crates `=<CLI version>`, so upgrade the `nsh` CLI and the app's `nsh-*` dependencies together.
- Headless test selectors: a text that contains `word:` must be written as `text:…`, otherwise the part before the colon is read as a selector prefix.
