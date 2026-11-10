---
type: decision
title: Agent guidance is a short always-loaded guide plus skills that setup installs
status: active
author: agent
confidence: high
created: 2026-11-10
updated: 2026-11-10
verified_commit: 3b0fc97
code_refs:
- crates/obmem-core/src/guide.rs#GUIDE
- crates/obmem-agents/src/setup.rs#SKILLS
- crates/obmem-agents/src/setup.rs#skills_dir
- crates/obmem-agents/src/setup.rs#write_skills
tags:
- agents
- skills
- architecture
approved_at: 2026-11-10
approved_by: auto-approve
---
# Agent guidance is a short always-loaded guide plus skills that setup installs

# Agent guidance is a short always-loaded guide plus skills that setup installs

## Context
One guide text told agents how to use obmem. It went through three channels: the MCP server instructions, the session-start context, and a block that `obmem setup` wrote into `AGENTS.md` for Cursor, Copilot CLI and Amp. All of it was in every session's context, so it had to stay short, and it could not hold a procedure such as checking stale notes against the code. On 2026-11-10 the user asked to move the guidance into skills.

## Decision
- The always-loaded guide (`guide::GUIDE`) keeps only what every task needs: what obmem is, `memory_context` before a task, `memory_log` before finishing, and one line that says when to load each skill. The user chose this over a guide that only points at a skill, and over keeping the full guide.
- The rest is in skills that ship in the binary and that `obmem setup <agent>` installs: `obmem` (searching, and what to record) and `obmem-refresh` (bringing stale notes back in line with the code). The user chose to ship the refresh skill with obmem instead of keeping it in this repo.
- Skills go to `.claude/skills/` for Claude Code and to `.agents/skills/` for Codex, Cursor, Copilot CLI and Amp. Those are the folders each agent's docs named on 2026-11-10; Claude Code's docs name only its own.
- The installed files are obmem's. `setup` rewrites one that differs from the build's copy, and `check` reports it. A new obmem version reaches a project that way.
- The `AGENTS.md` block and the `obmem guide` command are gone. Nothing removes an old block from a project's `AGENTS.md`.

## Alternatives considered
- Only a pointer in the always-loaded text: an agent that does not load the skill would neither read nor write memory.
- Keep the full guide and add skills: the same guidance in two places.
- One copy in `.agents/skills/` with a symlink from `.claude/skills/`: `atomic_write` refuses to write through a symlink, and a symlink in git is a text file on a Windows checkout without developer mode.
- Leave an edited skill file alone, as with the Amp plugin: a project would then keep an old skill for ever, and the skill describes obmem's own tools.

## Consequences
- A project set up for Claude Code and for another agent has two copies of each skill. Cursor and Copilot CLI also read `.claude/skills/`, so they may list a skill twice.
- A client that has no skills, or a project where `setup` never ran, gets the short guide and the tool descriptions. The guide then names skills that are not there.
- Changing a tool's name or behaviour means changing the skill that describes it, in the same commit.
- Checked on 2026-11-10: a new Claude Code session in this repo lists both skills. No other agent was tried, and no agent has run a task with them.

This repo also has a skill of its own, `linear-workflow` in `.agents/skills/`, linked into `.claude/skills/`. It is not part of obmem.
