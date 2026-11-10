---
type: fact
title: T3 Code's Cursor driver starts MCP servers in the home directory
status: active
author: agent
confidence: high
created: 2026-11-09
updated: 2026-11-09
verified_commit: 38a6b4e
code_refs:
- crates/obmem-mcp/src/lib.rs#use_project
- crates/obmem-core/src/store.rs#project_ready
tags:
- t3code
- cursor
- gotcha
- mcp
approved_at: 2026-11-09
approved_by: human (gui)
verified_at: 2026-11-10
verified_by: agent
---
# T3 Code's Cursor driver starts MCP servers in the home directory

Observed on 2026-11-09 with a logging wrapper around `obmem serve`. T3 Code runs Cursor children (`delegate_task` to provider `cursor`) through its own driver, which gives the MCP server no way to learn the project:
- **cwd** is the home directory (`/Users/<user>`), not the workspace.
- **`${workspaceFolder}`** in `.cursor/mcp.json` is passed literally, not interpolated.
- **No roots:** the client `Cursor 1.0.0` declares only `elicitation` and the `io.modelcontextprotocol/ui` extension.
- **No metadata:** `tools/call` carries no `_meta`.
- **`sessionStart`:** the `additional_context` from the `.cursor/hooks.json` hook does not reach the model. `AGENTS.md` does.

Codex children in T3 behave normally: the MCP server starts in the repo, and SessionStart context arrives.

How obmem copes:
- Without `.obmem.toml` it refuses project writes and explains itself instead of using `~/docs/kb`.
- Tools take an optional `project` argument. The guide (MCP instructions and `AGENTS.md`) tells agents to pass their workspace root, and a Cursor child did so unprompted.
- It ignores an uninterpolated `--project ${…}`.

The real fix is in T3: start MCP servers in the workspace, or interpolate `${workspaceFolder}`.

Related: [[obmem-in-a-git-worktree-cannot-reach-obsidian]].
