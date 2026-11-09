# obmem-agents

How obmem plugs into agent harnesses: the hook entry points (`hooks.rs`) and `obmem setup`, which writes each agent's config (`setup.rs`).

## Hooks

- A hook never breaks the agent's session. Errors go to stderr, and the hook answers with its agent's no-op form.
- The hooks find the project from the payload's `cwd` / `workspace_roots`. Without `.obmem.toml` there, every hook is a no-op, so installing them user-wide is harmless.
- `Agent` holds each agent's payload fields and output format:

| Agent | Session start | Stop (ask to log) | Before compaction |
| --- | --- | --- | --- |
| Claude Code | plain text | `{"decision":"block"}` | logs the session, steers the summary |
| Codex | plain text (also after compaction) | `{"decision":"block"}`; always answers JSON | no such event |
| Cursor | `additional_context` | `followup_message` | logs the session |
| Copilot CLI | `additionalContext` | `{"decision":"block"}` on `agentStop` | logs the session |
| Amp (plugin) | context on a thread's first turn | plugin `continue`, guarded by a marker | none |

- Read hook input field by field. Cursor sends both `conversation_id` and `session_id`, which breaks serde aliases. Claude-format hooks ignore Cursor payloads (`cursor_version` or `generation_id`). For captured Cursor payloads, run memory_search `cursor hook payloads`.
- Stop never loops. It lets the stop through when the agent is already continuing (`stop_hook_active`, or Cursor's `loop_count`), and it asks at most once per session every 30 minutes.
- Stop asks to log only after real work:
  - Claude Code: read from the transcript. Real work is an Edit / Write / MultiEdit / NotebookEdit call, or at least 5 tool calls since the last `memory_log`, because edits are often made through Bash.
  - Other agents: their transcripts are undocumented or unstable, so obmem uses git. Real work means repo files outside the vault that changed or were committed after the later of the session start and the last session-log entry. Writes to the vault do not count.
- Before compaction, the hook writes a mechanical log entry with secrets redacted.

## Setup

- `setup` merges into existing files and skips entries that are already there, so running it again is safe.
- `check` runs the same code without writing, and reports the files `setup` would change. Keep both behind the one `apply` flag so they cannot disagree.
- Generated commands call `obmem` from `PATH`, so the files can be committed. `setup cursor` writes `serve --project ${workspaceFolder}`.
- In `AGENTS.md`, only the block between `<!-- obmem:guide -->` and `<!-- /obmem:guide -->` is rewritten.

## Tested in the real apps

These were tested on 2026-11-09, both standalone and as T3 Code children: Codex 0.167 and cursor-agent 2026.11.05. The Amp plugin and the Copilot hooks follow those agents' docs but have not been run. Known limits:
- Codex loads project config and hooks only after the project and each hook are trusted. `codex exec` can call obmem's tools only with `mcp_servers.obmem.default_tools_approval_mode = "approve"`.
- `cursor-agent -p` needs `--force` for tool calls, and in that mode the stop hook did not fire.
- T3 Code's Cursor driver drops the `sessionStart` context. The guide block in `AGENTS.md` covers that case.
