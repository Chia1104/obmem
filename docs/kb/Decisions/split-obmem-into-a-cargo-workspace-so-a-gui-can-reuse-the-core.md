---
type: decision
title: Split obmem into a Cargo workspace so a GUI can reuse the core
status: active
author: agent
confidence: high
created: 2026-11-09
updated: 2026-11-09
verified_commit: '5662297'
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
approved_at: 2026-11-09
approved_by: human (gui)
---
**Context.** obmem was a single binary crate (16 modules, ~12.7k lines). The user plans a GUI settings and management app built with Native Shell (NSH, github.com/longbridge/nsh, nsh.dev). NSH Rust apps are ordinary Cargo crates with an `nsh.json` next to them, and they depend on `nsh-core` / `nsh-base` from crates.io. So the GUI needs obmem's logic as a library.

**Decision (2026-11-09, commit 5662297).** A virtual workspace (`members = ["crates/*"]`, shared `[workspace.package]` and `[workspace.dependencies]`):
- `obmem-core`: config, note, store, audit, catalog, context, drift, guide, import, init, obsidian, secrets. `audit` and `store` import each other, so they must stay in one crate.
- `obmem-mcp`: the MCP server. It is the only crate that depends on `rmcp`.
- `obmem-agents`: hooks and setup. The GUI's management screens will call `setup`.
- `obmem`: the clap binary. The package and binary names are unchanged (`cargo install --path crates/obmem`).
- `clap::ValueEnum` derives are behind an optional `clap` feature in core and agents. `schemars` stays a regular dependency, because the MCP schemas derive from core's enums.

**Alternatives.** Only core plus bin: this would put rmcp/tokio and hook code into the GUI's dependencies. One crate per module: boilerplate, and the audit/store cycle rules it out anyway.

**NSH GUI: verified on 2026-11-09 with nsh 0.4.0, in a scratch copy of the repo.**
- `nsh init apps/obmem-gui --template rust` writes a standalone crate with its own empty `[workspace]`. Both layouts work: standalone, or a member of the obmem workspace (remove the app's `[workspace]`, add `"apps/*"` to `members`, set `default-members = ["crates/*"]`). In both, `nsh build` and `nsh package --target macos` succeed, and a headless `nsh_testing::Headless` test in `cargo test` shows text computed with `obmem_core::config::Config::load`.
- This works because nsh writes its runners into `<app>/.nsh/rust/`, which is a Cargo workspace of its own with its own `Cargo.lock`, and it pulls the app crate in by absolute path. The outer workspace does not interfere.
- In member mode the root `Cargo.lock` grows from 142 to 592 packages. obmem's existing dependency versions did not change, and `default-members` keeps a root `cargo build` free of NSH.
- **Toolchain:** nsh-core 0.4.0 needs the latest stable Rust (it uses `if let` guards and `isolate_lowest_one`). It failed on 1.91.1 and builds on 1.99.1. NSH's repo pins `channel = "stable"`, not an MSRV. `huacnlee/cargo` is *not* required: it is only a cargo fork that shares the build dir across worktrees.
- The nsh CLI builds runners against NSH crates `=<CLI version>`, so upgrade the `nsh` CLI and the app's `nsh-*` dependencies together.
- Headless test selectors: a text that contains `word:` must be written as `text:…`, otherwise the part before the colon is read as a selector prefix.
- Still to decide: the real app id (the template's `com.example.*` is a placeholder), and how to run obmem-core's blocking calls (Obsidian CLI subprocesses) off the UI thread.
