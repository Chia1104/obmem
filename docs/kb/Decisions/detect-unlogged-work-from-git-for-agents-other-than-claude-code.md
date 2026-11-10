---
type: decision
title: Detect unlogged work from git for agents other than Claude Code
status: active
author: agent
confidence: high
created: 2026-11-09
updated: 2026-11-09
verified_commit: 38a6b4e
code_refs:
- crates/obmem-agents/src/hooks.rs#changed_since
- crates/obmem-agents/src/hooks.rs#Agent
- crates/obmem-agents/src/setup.rs#setup
tags:
- hooks
- agents
approved_at: 2026-11-09
approved_by: human (gui)
verified_at: 2026-11-10
verified_by: agent
---
# Detect unlogged work from git for agents other than Claude Code

## Context
The Stop hook asks the agent to `memory_log` only when the session did real work since its last log. For Claude Code, obmem reads the transcript (tool calls since the last `memory_log`). Codex, Cursor, Copilot CLI and Amp each have a different transcript format. Codex's docs say the format "isn't a stable interface for hooks and may change over time", and Cursor and Amp don't document theirs.

## Decision
For every agent except Claude Code, "real work" means: files in the repo, outside the project vault, that were modified or committed after the later of
- the session start (recorded by the session-start hook in the hook-state dir), and
- the newest session-log file.

Vault writes (`memory_write`) don't count.

## Alternatives considered
- One transcript parser per agent: four undocumented formats that can break at any release.
- Always ask at stop: annoying, and agents learn to ignore it.

## Consequences
- Work that doesn't touch files (pure research, answering questions) isn't detected for those agents.
- Edits made before the session started never count.
- Agent-specific output formats live in `Agent` (`crates/obmem-agents/src/hooks.rs`), and `obmem setup` writes each agent's config.

Related: [[use-the-obsidian-cli-for-search-and-link-checks-instead-of-an-own-index]].
