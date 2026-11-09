---
type: finding
title: Cursor may also run the Claude Code hooks in .claude/settings.json
status: fixed
author: agent
confidence: medium
created: 2026-11-09
updated: 2026-11-09
verified_commit: 8b260e5
code_refs:
- crates/obmem-agents/src/hooks.rs#HookInput
- crates/obmem-agents/src/setup.rs#setup
tags:
- hooks
- cursor
- multi-agent
---
# Cursor may also run the Claude Code hooks in .claude/settings.json

herdr (github.com/herdrdev/herdr, src/integration/assets/claude/herdr-agent-state.sh) has its Claude hook exit early when `CURSOR_VERSION` is in the environment or `cursor_version` is in the payload. That guard means Cursor runs Claude Code hooks it finds in Claude's settings.

obmem ships both `.claude/settings.json` (`obmem hook session-start`, no `--agent`, so Claude format) and `.cursor/hooks.json` (`--agent cursor`). If Cursor loads both, a Cursor session could get its session-start context twice, and get a stop hook in Claude's `{"decision":"block"}` format, which Cursor does not use (it expects `followup_message`).

Not yet checked against a real Cursor session. Possible fix: in `obmem hook` with the default agent (Claude), treat `CURSOR_VERSION` or `cursor_version` as "this is Cursor" and either do nothing or switch to the Cursor format.

Related: herdr's Claude hook also skips SessionStart payloads that carry `agent_id` (subagents).
- 2026-11-09: open → open by agent: 2026-11-09: not reproduced in T3 Code's Cursor driver (composer-2.5, CURSOR_VERSION=1.0.0). Logging hooks in both config files showed that only .cursor/hooks.json runs (sessionStart and stop). With .cursor/hooks.json removed, no hook ran, so there is no fallback to the .claude/settings.json hooks. Not checked: the Cursor IDE, a third-party-config setting, and the user-level ~/.claude/settings.json. Cursor does set CLAUDE_PROJECT_DIR. Side results: the Cursor stop hook fires in T3, and its payload has a transcript_path (~/.cursor/projects/<slug>/agent-transcripts/<id>/<id>.jsonl). Kept open for a human: reject it, or test it in the IDE.
- 2026-11-09: open → fixed by agent: Guard added, not committed yet: `HookInput::misrouted` in src/hooks.rs, checked first in `run_hook` in src/main.rs. A Claude-format hook (default agent) does nothing when the payload has `cursor_version` (T3 driver) or `generation_id` (cursor-agent 2026.11.05 omits cursor_version). The guard reads only the payload, never the CURSOR_VERSION env var, because Claude Code started from Cursor's terminal could inherit that var. Unit tests and an end-to-end run of the binary pass. The installed ~/.cargo/bin/obmem gets it after `cargo install --path .`.
