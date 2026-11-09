---
type: decision
title: 'obmem GUI scope: control panel for macOS and Windows, notes stay in Obsidian'
status: draft
author: agent
confidence: high
created: 2026-11-09
updated: 2026-11-09
verified_commit: 916359b
code_refs:
- crates/obmem-core/src/projects.rs
- crates/obmem-core/src/status.rs#status
- crates/obmem-core/src/store.rs#approve
- crates/obmem-agents/src/setup.rs#check
- apps/obmem-gui/src/state.rs
tags:
- gui
- nsh
- windows
- tray
---
Decided with the user on 2026-11-09. This builds on [[split-obmem-into-a-cargo-workspace-so-a-gui-can-reuse-the-core]].

**Scope.** The NSH GUI (`apps/obmem-gui`) is a control panel for what today needs the CLI or hand-edited config. Notes are read and edited in Obsidian, and the GUI deep-links there instead of building a note editor or a search UI. It is window-only: no tray yet.

**Choices**
- **Platforms:** macOS and Windows.
- **Project list:** both automatic and manual. The session-start hook and `obmem init` record each project in `~/.config/obmem/projects.toml`, and the GUI or `obmem projects add` can add a folder by hand.
- **Drafts:** a person can approve them (draft → active) with `Store::approve`, through `obmem approve` or the GUI. MCP does not offer it.
- **Scheduled audit:** not now.

**Layers** (build each on a working one):
1. Done on 2026-11-09 (branch `feat/gui-layer1`): project list, health checks (version of the `obmem` on PATH, vaults existing and registered in Obsidian, draft and finding counts), agent setup with a read-only `setup::check`, and initializing a folder. Covered by a headless flow test.
2. Settings forms (`.obmem.toml`, global config, the Codex auto-approve switch; write with `toml_edit` so comments survive) and the secret allowlist.
3. A review inbox for drafts and findings (approve, resolve, verify), plus running the audit.
4. A preview of what an agent sees for a task (memory_context / session start), the session timeline, and import-claude.

**Tray.** NSH 0.4.0 has none. NSH `main` has `Tray` in `nsh/desktop` for TSX only; the engine's `DesktopCommand::SetTray` exists. Add the tray once a release exposes it in Rust, or upstream a `set_tray()` modelled on `set_application_menu`.

**Windows.** Unix-only symlink tests are now `cfg(unix)`. Still unverified on a Windows machine: the Obsidian CLI's name and install, the GUI build, and `nsh package --target windows`.
