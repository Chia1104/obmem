---
type: fact
title: obmem in a git worktree cannot reach Obsidian
status: active
author: agent
confidence: medium
created: 2026-11-09
updated: 2026-11-10
verified_commit: 38a6b4e
code_refs:
- crates/obmem-core/src/obsidian.rs#vault_id
tags:
- obsidian
- gotcha
- worktree
approved_at: 2026-11-10
approved_by: auto-approve
verified_at: 2026-11-09
verified_by: agent
---
# obmem in a git worktree cannot reach Obsidian

obmem finds a vault's Obsidian id by its **canonical path** in `obsidian.json`. A git worktree (for example one made by T3 Code `isolation: worktree`, or by `git worktree add`) has its own copy of `docs/kb` at a different path, and Obsidian has never registered that copy. So inside a worktree:
- `memory_search`, `memory_context` and backlinks report the project vault as "not an Obsidian vault yet".
- The audit's `broken-link` and `orphan` rules are skipped.

Writes still land in the worktree's `docs/kb` and merge back with the branch. The SessionStart context also still works, since it reads files.

This applies only where the `obsidian` command is on `PATH`. A machine without it uses the built-in search, which reads the files of whatever folder it is given, so a worktree works there. See [[a-headless-backend-answers-for-obsidian-where-the-app-is-not-installed]].

**How to apply:** run agents that need memory search in the main checkout (T3 `isolation: shared`). Or accept degraded search in worktrees; registering every worktree as a vault is not worth it.

See [[obsidian-cli-behaviour]].
