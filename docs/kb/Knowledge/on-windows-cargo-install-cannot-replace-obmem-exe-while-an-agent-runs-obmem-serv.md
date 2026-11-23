---
type: fact
title: On Windows cargo install cannot replace obmem.exe while an agent runs obmem serve
status: active
author: agent
confidence: medium
created: 2026-11-17
updated: 2026-11-17
tags:
- windows
- install
- gotcha
approved_at: 2026-11-17
approved_by: auto-approve
---
# On Windows cargo install cannot replace obmem.exe while an agent runs obmem serve

On Windows, `cargo install --path crates/obmem` builds fine and then fails with `failed to move ... cargo-installXXXX\obmem.exe to ...\.cargo\bin\obmem.exe` and `os error 5` (access denied). Windows does not let a running `.exe` be overwritten, and every agent session (Claude Code, Codex, Cursor, T3 Code) keeps an `obmem serve` MCP server running from that file.

Seen on 2026-11-17: two `obmem serve` processes, both started by `claude.exe`.

Two ways through:
- Close every agent session, check with `Get-Process obmem` that none is left, then install.
- Or rename the running file first. Windows allows renaming a running executable: `Rename-Item $env:USERPROFILE\.cargo\bin\obmem.exe obmem.old.exe`, then `cargo install --path crates/obmem`. The old servers keep running from `obmem.old.exe`, and new sessions start the new build. Delete `obmem.old.exe` once those sessions are closed.

macOS and Linux replace a running binary without trouble, so this happens only on Windows.
