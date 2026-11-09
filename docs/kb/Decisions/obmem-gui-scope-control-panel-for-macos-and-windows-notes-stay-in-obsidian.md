---
type: decision
title: 'obmem GUI scope: control panel for macOS and Windows, notes stay in Obsidian'
status: draft
author: agent
confidence: high
created: 2026-11-09
updated: 2026-11-09
verified_commit: 89275cd
code_refs:
- crates/obmem-core/src/projects.rs
- crates/obmem-core/src/status.rs#status
- crates/obmem-core/src/settings.rs
- crates/obmem-core/src/review.rs
- crates/obmem-core/src/store.rs#NoteRef
- crates/obmem-agents/src/setup.rs#Writer
- apps/obmem-gui/src/state.rs
tags:
- gui
- nsh
- windows
- tray
---
Decided with the user on 2026-11-09. This builds on [[split-obmem-into-a-cargo-workspace-so-a-gui-can-reuse-the-core]].

**Scope.** The NSH GUI (`apps/obmem-gui`) is a control panel for what today needs the CLI or hand-edited config. Notes are read and edited in Obsidian, and the GUI deep-links there (`obsidian::open_uri`) instead of building a note editor or a search UI. It is window-only: no tray yet.

**Choices**
- **Platforms:** macOS and Windows.
- **Project list:** both automatic and manual. The session-start hook and `obmem init` record each project in `~/.config/obmem/projects.toml`, and the GUI or `obmem projects add` can add a folder by hand.
- **Drafts:** a person can approve them (draft → active) with `Store::approve`, through `obmem approve` or the GUI. MCP does not offer it.
- **Scheduled audit:** not now.
- **Codex auto-approval:** off by default, with a per-project switch (CHI-59, closed).
- **Languages:** Traditional Chinese and English, following the system, with English when neither matches. Waits for NSH to release i18n (CHI-66).

**Layers, all built on 2026-11-09:**
1. Project list, health checks, agent setup with `setup::check`, initializing a folder.
2. Settings (`.obmem.toml` and the user's config through `toml_edit`, keeping comments; the Codex switch) and secrets (try the scanner, allow a false positive in the project or personal allowlist).
3. A cross-project review screen: approve, resolve, verify the target note, open in Obsidian, run the audit.
4. Agent view (session-start context and `memory_context` for a task, shown as plain text exactly as an agent gets it), session logs, and importing Claude Code memory with a per-item choice.

Each layer has a headless flow test (`apps/obmem-gui/tests/*.rs`, one test per file, since each file sets its own environment).

**Security lessons from these layers.** A cloned project may plant symlinks or notes. Setup writes go through `atomic_write`. Review actions name notes with `NoteRef::At`, an exact path in a given vault, never with a key taken from a note's content.

**Tray.** NSH 0.4.0 has none. NSH `main` has `Tray` (TSX only); the engine's `DesktopCommand::SetTray` exists. Add it once a release exposes it in Rust, or upstream a `set_tray()`.

**Windows.** Unverified on a Windows machine: the Obsidian CLI's name and install, the GUI build, and `nsh package --target windows`.
