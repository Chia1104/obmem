# obmem-agents

How obmem plugs into agent harnesses: the hook entry points (`hooks.rs`), `obmem setup`, which writes each agent's config (`setup.rs`), and the skills it installs (`skills/`).

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
- The Amp plugin runs the stop hook at every turn end, also at the end of the turn it started itself with `continue`. That is the turn in which the agent writes its log, and the sync that carries the log belongs to the stop hook. The hook cannot ask twice (the 30 minute rule), and the plugin ignores its answer in a turn that carries the marker.
- The Amp plugin sends a hook's stderr to the plugin log (`amp.logger`). In an orb nothing else shows a failed sync.

## Setup

- `setup` merges into existing files and skips entries that are already there, so running it again is safe.
- `check` runs the same code without writing, and reports the files `setup` would change. Keep both behind the one `Writer` so they cannot disagree.
- A step only the user can take (a trust prompt, an app setting) is a `ManualStep` in `Report::manual`. Its `Display` is the instruction the CLI prints; the desktop app translates it, so a new step needs a case in its `words::manual`.
- Every write goes through `Writer::write` (`atomic_write`): the project may be an untrusted clone, and a config file planted as a symlink must not redirect a write outside it.
- Generated commands call `obmem` from `PATH`, so the files can be committed. `setup cursor` writes `serve --project ${workspaceFolder}`.
- `setup amp` writes the MCP server to the workspace settings (`amp.mcpServers` in `.amp/settings.json`), never to the user's. An orb, Amp's remote machine, has the repository and nothing from the user's own settings file. Amp runs a workspace server only after the user approves it.
- The Amp plugin is written only when `.amp/plugins/obmem.ts` is missing, because people edit it. After changing `AMP_PLUGIN`, delete the file and run `setup amp` again.
- `setup codex` leaves Codex asking before each obmem tool call. `set_codex_auto_approve` turns on `default_tools_approval_mode = "approve"` per project (the GUI's switch); `codex exec` needs it.

## Skills

What an agent is told about obmem is in two layers:

- `obmem_core::guide::GUIDE` is in every session's context (the MCP server instructions and the session-start context). It holds what every task needs: the call that opens a task, the call that closes it, and one line that says when to load each skill.
- `skills/<name>/SKILL.md` holds the rest. An agent loads a skill when its `description` fits, so the description names the cases that should load it. `obmem` is how to search and what to record. `obmem-refresh` is the procedure that brings stale notes back in line with the code.

Rules for changing them:

- `setup` installs every skill in `SKILLS` (`setup.rs`): into `.claude/skills/` for Claude Code, which reads only that folder, and into `.agents/skills/` for Codex, Cursor, Copilot CLI and Amp.
- The installed files are obmem's. `setup` rewrites one that differs from this build's copy, and `check` reports it as a file to update. That is how a project gets a new version of a skill.
- A skill's `name` is its folder's name, or agents do not load it. A test checks it.
- A new skill needs a line in `GUIDE` only when an agent must reach it without being asked. Every word there is paid for in every session.
- Tool names and behaviour that a skill describes are in obmem-mcp and obmem-core. When one changes, change the skill in the same commit.

Where each agent reads skills was taken from its docs on 2026-11-10. Only Claude Code was checked: a new session lists both skills. No agent has run a task with them yet.

## Tested in the real apps

These were tested on 2026-11-09, both standalone and as T3 Code children: Codex 0.167 and cursor-agent 2026.11.05. The Copilot hooks follow that agent's docs but have not been run. For Amp (CLI of 2026-11-08), `amp plugins list` loads the plugin with both events and `amp mcp doctor` finds the server in `.amp/settings.json`, awaiting approval. No Amp thread has run with them, and nothing was tried in an orb. Known limits:
- Codex loads project config and hooks only after the project and each hook are trusted. `codex exec` can call obmem's tools only with `mcp_servers.obmem.default_tools_approval_mode = "approve"`.
- `cursor-agent -p` needs `--force` for tool calls, and in that mode the stop hook did not fire.
- T3 Code's Cursor driver drops the `sessionStart` context. The MCP server instructions carry the same guide, so the agent still gets it.
