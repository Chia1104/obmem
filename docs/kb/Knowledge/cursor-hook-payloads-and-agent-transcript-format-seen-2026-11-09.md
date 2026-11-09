---
type: fact
title: Cursor hook payloads and agent transcript format (seen 2026-11-09)
status: draft
author: agent
confidence: medium
created: 2026-11-09
updated: 2026-11-09
verified_commit: 8b260e5
code_refs:
- crates/obmem-agents/src/hooks.rs#HookInput
- crates/obmem-agents/src/hooks.rs#scan_transcript
- crates/obmem-agents/src/hooks.rs#stop
tags:
- cursor
- hooks
- multi-agent
---
# Cursor hook payloads and agent transcript format (seen 2026-11-09)

Seen with T3 Code's Cursor driver (composer-2.5, `cursor_version` 1.0.0). None of this is documented by Cursor, so treat it as unstable.

**Hook payloads.** Every hook sends `conversation_id`, `generation_id` and `session_id` (all three the same value), plus `model`, `cursor_version`, `workspace_roots` and `transcript_path`. In `sessionStart`, `transcript_path` is null. `stop` also has `status` ("completed") and `loop_count`, and its `transcript_path` is set. cursor-agent 2026.11.05 did not send `cursor_version`, but did send `generation_id`. Claude Code sends neither. Cursor also sets the `CLAUDE_PROJECT_DIR` env var for hooks.

**Transcript.** The file is `~/.cursor/projects/<path-slug>/agent-transcripts/<id>/<id>.jsonl`. Each line is `{"role": "user"|"assistant", "message": {"content": [...]}}`:
- Parts are `text` or `tool_use` (`name`, `input`). There are no tool results, no per-line timestamps (only a `<timestamp>` inside user text) and no sidechains.
- Assistant reasoning appears as "[REDACTED]".
- File edits are `Write` with `path` (other edit tools are probably separate tools). Shell commands are `Shell` with `command`.
- MCP calls are `CallDynamicTool` with `namespace` (the server, e.g. "obmem"), `toolName` and `arguments`. Before calling a tool the agent may call `GetDynamicTools` to fetch its schema.
- A stop-hook `followup_message` shows up as a new user turn inside `<user_query>`.

So the transcript can tell whether `memory_log` was called since the last edit, as obmem already does for Claude Code transcripts.
