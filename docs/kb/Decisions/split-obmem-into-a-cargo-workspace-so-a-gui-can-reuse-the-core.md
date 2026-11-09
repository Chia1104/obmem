---
type: decision
title: Split obmem into a Cargo workspace so a GUI can reuse the core
status: draft
author: agent
confidence: high
created: 2026-11-09
updated: 2026-11-09
verified_commit: bc5d85f
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
---
# Split obmem into a Cargo workspace so a GUI can reuse the core

**Context.** obmem was a single binary crate (16 modules, ~12.7k lines). The user plans a GUI settings and management app built with Native Shell (NSH, github.com/longbridge/nsh, nsh.dev). NSH Rust apps are ordinary Cargo crates with an `nsh.json` next to them, they depend on `nsh-core` / `nsh-base` from crates.io, and they can use path dependencies (`shared = { path = "../shared" }`). So the GUI needs obmem's logic as a library.

**Decision (2026-11-09).** A virtual workspace (`members = ["crates/*"]`, shared `[workspace.package]` and `[workspace.dependencies]`):
- `obmem-core`: config, note, store, audit, catalog, context, drift, guide, import, init, obsidian, secrets. `audit` and `store` import each other, so they must stay in one crate.
- `obmem-mcp`: the MCP server (formerly `src/server.rs`). It is the only crate that depends on `rmcp`.
- `obmem-agents`: hooks and setup. The GUI's management screens will call `setup`.
- `obmem`: the clap binary. The package and binary names are unchanged, so hook configs and `cargo install` keep working (`cargo install --path crates/obmem`).
- `clap::ValueEnum` derives are behind an optional `clap` feature in core and agents (`cfg_attr`). Only the CLI turns it on. `schemars` stays a regular dependency, because the MCP tool schemas derive from core's enums.
- `audit_text` moved from main.rs to `obmem_core::audit::report_text`, because both the CLI and MCP use it.

**Alternatives.** Only core plus bin: this would put rmcp/tokio and hook code into the GUI's dependencies. One crate per module: a lot of boilerplate for no benefit, and the audit/store cycle rules it out anyway.

**For the GUI (not done yet).** Put it at `apps/obmem-gui` (an NSH Rust app with `nsh.json`). It should depend on `obmem-core` and `obmem-agents` without `clap`. NSH pulls in about 60 crates plus platform libraries (GTK 4 on Linux). If the GUI is a workspace member, set `default-members` to the `crates/*` so that `cargo build` / `cargo test` at the root stay light, or `exclude` it. Unverified: whether `nsh run` / `nsh package` handle an app crate that is a member of an outer workspace. NSH's README also says the Rust toolchain is "built with huacnlee/cargo" (a cargo fork). Check both when the GUI is scaffolded.
