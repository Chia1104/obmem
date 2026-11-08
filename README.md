# obmem — Obsidian-backed memory for coding agents

A single Rust binary: an MCP server (stdio, [rmcp](https://github.com/modelcontextprotocol/rust-sdk) 3.9) plus a CLI.
Agents read and write plain Markdown files directly; Obsidian is the UI people use to browse and review them.
obmem has no search index of its own. Search, backlinks, unresolved links and orphans come from Obsidian itself through the [official Obsidian CLI](https://obsidian.md/help/cli), so agents get the same results as the app.

## Two vaults

| Vault | Default location | What goes in it |
|---|---|---|
| project | `<repo>/docs/kb` (git, shared with the team) | decision / fact / concept / architecture / procedure / finding |
| global | `~/Obsidian/AgentMemory` (personal) | feedback / preference (`Global/`), session logs (`Projects/<name>/Sessions`) |

## Obsidian setup (required)

1. Obsidian 1.12 or later: Settings → General → enable **Command line interface**.
2. "Open folder as vault" once for each vault folder. Obsidian cannot register vaults from the CLI or a URI, so this step is manual. `obmem status` shows whether each vault is registered.

obmem looks up each vault's id by path in Obsidian's `obsidian.json` and always passes `vault=<id>`. Run from an unregistered folder, the CLI silently uses the *active* vault, and every repo's vault is named `kb`, so the id is the only safe handle.

If Obsidian is closed, the CLI fails with "unable to find Obsidian", even though the docs say it starts the app. obmem then starts Obsidian in the background (`open -g -a Obsidian` on macOS), waits for its commands to register (about 1-2 s) and retries. If a vault is not registered or the CLI is missing, obmem keeps working with less:
- Search covers the other vault and says which one it skipped.
- `memory_get` returns the note with "backlinks: unavailable".
- The audit skips only `broken-link` and `orphan`, with a note in the report.

Writes, the SessionStart hook and the other audit rules read the files directly and never need Obsidian.

## Quick start

```sh
cargo install --path .
cd your-repo && obmem init        # creates both vaults and config, prints MCP/hook setup snippets
obmem status
```

## This repo uses obmem itself

This repo dogfoods obmem:
- `.mcp.json` registers the server.
- `.claude/settings.json` holds the three hooks.
- `.githooks/pre-commit` runs `obmem scan --staged`; enable it with `git config core.hooksPath .githooks`.
- `docs/kb` is the project vault.

All of these call `obmem` from `PATH`, so run `cargo install --path .` first.

## MCP tools

- `memory_search`: Obsidian search across both vaults. The syntax is the same as the app's search box: words are ANDed, plus `OR`, `"phrase"`, `-word`, `tag:`, `path:`, `[property:value]` and `line:( )`. obmem wraps the query in parentheses and appends `[type:…]` (when `type` is given) plus `-[status:archived] -[status:superseded] -path:Templates -path:_system`. Results are not relevance-ranked. Snippets are matching body lines; frontmatter matches are skipped.
- `memory_get`: full note text read from disk, plus backlinks from Obsidian.
- `memory_context`: the notes most relevant to a task description, in any language.
  - Turns the task into an Obsidian `OR` query. Terms are cut by script, not by language, so every language gets terms without a word list:
    - **Scripts written with spaces** (Latin with accents, Cyrillic, Greek, Arabic, Hebrew, Indic, …): words of 3 or more characters, with combining marks kept inside the word.
    - **Han:** character bigrams.
    - **Japanese kana:** a Katakana run is kept whole (トークン); Hiragana is dropped because it is mostly particles.
    - **Hangul:** bigrams, because particles attach to nouns.
    - **Thai, Lao, Khmer and Myanmar:** bigrams of character clusters.
    - Small English and Chinese filler lists remove words like `the` and 幫我. When there are more than 16 terms, the longest ones win.
  - Ranks the hits by how many distinct terms each note contains (title matches count double), because Obsidian search does not rank.
  - Lists open findings about the notes it returns.
  - Matching is literal: "rotation" does not find a note that says 輪替, and an inflected form (ротацию) finds only itself, not ротация.
- `memory_write`: create or update a note. Every write goes through these checks:
  - refuses near-duplicate titles (trigram similarity ≥ 0.5) and points at `update_id`
  - human-authored notes (`author` not `agent:*`) are read-only; write a `finding` instead
  - new notes get `status: draft`; an agent edit of an `active` note puts it back to `draft` for
    re-review; `superseded` / `archived` notes cannot be edited in place
  - `update_id` must match exactly one note
  - `code_refs` writes record the current `verified_commit`
  - secret scanning of every persisted field: title, body, tags, code_refs, confidence (see below)
- `memory_log`: append to today's session log.
- `memory_audit`: run the rule-based audit (see below); `write_findings: true` also writes finding notes.
- `memory_resolve`: mark a finding `fixed`, `accepted` or `open` with a comment. Agents cannot reject findings.
- `memory_verify`: after re-reading the referenced code, record that an agent-authored note is still accurate (`verified_commit` = HEAD, `verified_at` = today). Refused while any of its `code_refs` is broken.

## Agent usage guide

What to record and what not to record lives in one text, delivered three ways:
- as the MCP server `instructions` that every MCP client receives
- inside the SessionStart context
- through `obmem guide >> AGENTS.md` for agents that use neither

## Importing Claude Code memory

`obmem import-claude [--dir <folder>] [--dry-run]` imports `~/.claude/projects/<project>/memory/*.md` as draft notes (author `agent:import`). `MEMORY.md` is skipped because it is only the index.

| Claude memory type | Becomes | In |
|---|---|---|
| `user` | preference | global vault |
| `feedback` | feedback | global vault |
| `project` | fact | project vault |
| `reference` | fact, tagged `reference` | project vault |

- **Name and links:** each note keeps the memory's `name` as its filename, so `[[name]]` links between memories still resolve, and the `description` becomes the title.
- **Re-running:** files that already exist are skipped, so the import is safe to run again.
- **What lands in the repo:** `project` memories go to the repo vault. Check them for machine-specific details before committing.

## Agents

`obmem setup <agent>...` registers the MCP server and the lifecycle hooks in that agent's project config. It merges into existing files, never overwrites them, and skips entries that are already there, so running it again is safe. Commands call `obmem` from `PATH`, so the files can be committed.

The hooks do nothing in a project without `.obmem.toml`, so installing them user-wide is harmless. They read the project from the payload's `cwd` / `workspace_roots`.

| Agent | Files `setup` writes | Session start | Stop (ask to log) | Before compaction |
|---|---|---|---|---|
| Claude Code | `.mcp.json`, `.claude/settings.json` | plain text | `{"decision":"block"}` | logs the session, steers the summary |
| Codex | `.codex/config.toml`, `.codex/hooks.json` | plain text (also runs after compaction) | `{"decision":"block"}`; always answers JSON | no such event |
| Cursor (IDE, `cursor-agent`) | `.cursor/mcp.json`, `.cursor/hooks.json`, `AGENTS.md` | `additional_context` | `followup_message` (guarded by `loop_count`) | logs the session |
| GitHub Copilot CLI | `.github/hooks/obmem.json`, `AGENTS.md`; MCP in `~/.copilot/mcp-config.json` by hand | `additionalContext` | `{"decision":"block"}` on `agentStop` | logs the session |
| Amp | `.amp/plugins/obmem.ts`, `AGENTS.md`; MCP with `amp mcp add obmem --workspace -- obmem serve` | plugin injects the context on a thread's first turn | plugin `continue`, guarded by a marker | — |

Every MCP client also receives the usage guide as server instructions. Agents that read `AGENTS.md` get it there too.

**What the session start shows:** the Index, the latest session-log entries ("where the last session left off"), global feedback and preferences, open findings, and unreviewed drafts.

**When "stop" asks the agent to log:**
- Claude Code: from the transcript. That means an Edit / Write / MultiEdit / NotebookEdit call, or at least 5 tool calls since the last `memory_log`, because edits are often made through Bash.
- Other agents: their transcripts are undocumented, and Codex calls its own format unstable. So obmem checks whether repo files outside the vault changed or were committed after the later of two times: when the session started, or the last session-log entry. Writing to the vault does not count as work.

**It never loops:**
- It lets the stop through when the agent says it is already continuing (`stop_hook_active`, or Cursor's `loop_count`).
- It asks at most once per session every 30 minutes.

**Before compaction:** obmem writes a mechanical log entry with secrets redacted. For Claude Code it lists recent requests and edited files. For other agents it lists the changed files.

**Errors:** every hook prints errors to stderr and answers the agent's "no-op" form, so a hook problem never breaks the session.

**Wrong working directory:** some MCP hosts start the server outside the project. T3 Code's Cursor driver uses the home directory, does not interpolate `${workspaceFolder}` and offers no roots. obmem handles this in four ways:
- **Refuses guessed project writes.** If the resolved project has no `.obmem.toml`, it refuses project-vault writes and says so, instead of writing to a guessed `~/docs/kb`.
- **Asks the client.** It asks the client for MCP roots when the client supports them.
- **Takes a `project` argument.** Every tool accepts an optional `project`: the agent's absolute workspace root.
  - It only *binds* a server that has no project yet. The path must have `.obmem.toml`, and obmem keeps it for the session.
  - A server that already has a project (from its cwd, `--project`, roots or an earlier `project`) is never switched to another one. A misled agent cannot redirect writes into a different project's vault. Paths are canonicalized before comparing.
  - The guide tells agents to pass it.
- **Ignores `--project ${…}`.** If a host passes the variable uninterpolated, obmem falls back to the cwd.

`setup cursor` writes `serve --project ${workspaceFolder}`. `setup` refreshes the guide block in `AGENTS.md` in place.

**Tested against the real apps (2026-11-09):** Codex 0.167 and cursor-agent 2026.11.05. In both, the session-start context reaches the model, and `memory_search` / `memory_context` work. Also tested as T3 Code children. Codex works fully. In Cursor, `sessionStart` context does not arrive (`AGENTS.md` does), and the agent passed `project` on its own after reading the guide.

**Caveats:**
- **Codex project config:** Codex loads project config and hooks only after the project and each hook are trusted.
- **Codex tool approval:** obmem's tools need approval like any MCP tool. `codex exec` runs with approval policy `never`, so it can call them only with `mcp_servers.obmem.default_tools_approval_mode = "approve"`, set in config or with `-c`.
- **`cursor-agent -p`:** tool calls need `--force`; `--approve-mcps` only enables the server. In this non-interactive mode, the `stop` hook did not fire.
- **Cursor payload:** Cursor sends both `conversation_id` and `session_id`, so obmem reads hook input field by field rather than through serde aliases.
- **Cursor `sessionStart`:** some Cursor IDE builds are reported to ignore `sessionStart` context. The `AGENTS.md` guide covers that case.
- **Untested:** the Amp plugin and the Copilot hooks follow the docs but have not been run in those apps.

## Audit (找碴, P1)

`obmem audit` runs deterministic checks over both vaults. They use no LLM and no tokens.

| Rule | Severity | Catches |
|---|---|---|
| `broken-link` | warn (info if the target exists only in the other vault) | Obsidian's `unresolved` links. obmem adds fix hints: a link written with a title instead of the filename gets the exact `[[file\|title]]` fix, and a link to a note in the other vault is called out |
| `schema` | error / warn / info | unparseable frontmatter, unknown `type`, invalid `status` for the type, bad `confidence`, non-`YYYY-MM-DD` dates, `code_refs` without `verified_commit` |
| `stale` | warn | `active` knowledge notes whose `verified_at` / `updated` / `created` is older than `stale_days` (180) |
| `supersede` | warn | active notes that still link to a `superseded` note; names the replacement if one has `supersedes:` |
| `duplicate` | warn | same-type titles with trigram similarity ≥ `duplicate_threshold` (0.75) |
| `orphan` | info | Obsidian's `orphans`, narrowed to active/draft knowledge notes (not Index) |
| `code-drift` | warn / info | project-vault `code_refs` that no longer exist, whose `#symbol` vanished, or whose code changed since `verified_commit` (see below) |

```sh
obmem audit                       # report >= warn
obmem audit --min-severity info   # include orphans and cross-vault links
obmem audit --write               # create / close / reopen finding notes
obmem audit --fail-on error       # exit 1 if any error
obmem audit --json --rule schema
obmem resolve <finding-id> rejected --comment "intentional"
obmem verify <note-id>            # re-checked against current code → verified_commit = HEAD
```

`--write` keeps findings in sync with reality:
- A new problem creates `Findings/audit-<rule>-<note>-<fp>.md` with `status: open`.
- It closes a finding automatically once its problem is gone.
- It reopens a `fixed` finding if the problem comes back.
- When a human sets a finding to `rejected`, in Obsidian or with `obmem resolve`, its fingerprint goes into `_system/audit-suppressions.toml`. That problem is never reported again, even after the finding note is deleted.

Findings name their note by path, never with a `[[link]]`. Obsidian counts links as backlinks, so a finding that linked its note would stop that note from being an orphan.

The same file can also disable rules (`disabled_rules`) or skip paths (`ignore_paths`) for that vault. `Templates/` and `_system/` are always skipped. Review findings in `_system/Findings.base`. Open findings also appear in the SessionStart context. Tune `stale_days` and `duplicate_threshold` under `[audit]` in `.obmem.toml`.

## Code drift (P2)

`code_refs` anchor a note to code. The supported forms are:
- `src/auth/refresh.rs#rotate_token`
- `src/auth/refresh.rs#Store::rotate` (every segment must appear)
- `src/auth/refresh.rs:42` and `:L10-L20` (the line part is ignored)
- `src/auth/` (a directory)

Paths are relative to the project root. `memory_write` refuses paths that leave the repo, warns about refs that don't resolve, and records `verified_commit` = HEAD.

`code-drift` checks each ref of an active or draft project-vault note:

| Problem | Severity |
|---|---|
| path gone (names the deleting commit) | warn |
| `#symbol` no longer in the file | warn |
| path changed since `verified_commit`, and the changed lines or their 3-line context mention the symbol (or the ref has no symbol) | warn |
| path changed, but the diff doesn't mention the symbol (probably still accurate) | info |
| `verified_commit` unknown to git (rebased away, shallow clone) | info |

Each finding's fix names the exact `git diff <commit> HEAD -- <path>` to read. After re-checking, run `obmem verify <id>` (humans, any note) or `memory_verify` (agents, own notes). `verified_at` also resets `stale`. The fingerprint includes the commit, so the next `audit --write` closes the old finding automatically.

## Secret scanning (no long hard-coded list to maintain)

1. **Known values**: the real values in `.env*` plus env vars whose names look sensitive, matched exactly with Aho-Corasick. Works for any format, including in-house tokens, with essentially zero false positives. Values stay in memory only.
2. **Format rules**: [`src/secret_rules.toml`](src/secret_rules.toml) has only ~15 rules for fixed-prefix tokens. It uses the gitleaks format, so you can drop a full upstream `gitleaks.toml` into `_system/secret-rules.toml`.
3. **Context + entropy**: `password = <value>`-style assignments where the value's Shannon entropy is ≥ 3.5, which filters out `changeme`.

Placeholders like `<YOUR_KEY>`, `${VAR}`, `xxxx` are ignored. False positives: copy the `fingerprint` from the error into `_system/secret-allowlist.toml`.
`mode = "block"` (default) refuses the write and tells the agent to replace the value with a reference; `"redact"` writes `[REDACTED:rule]` instead.
`obmem scan --staged` works as a git pre-commit hook, so human edits to the vault get checked too. It scans only staged files inside the project vault; source code is out of scope (test fixtures contain fake tokens).

The project vault may come from an untrusted clone, so its files can only add to the scanner, never weaken it:
- Vault rule files cannot override built-in rule ids.
- Allowlist files cannot add rules.
- Project-vault allowlists contribute exact `fingerprints` only. Broad entries (`regexes`, `stopwords`) count only in the personal global vault's `_system/secret-allowlist.toml`.
- Even there, stopwords shorter than 4 characters and regexes that match almost anything are ignored.

For the same reason:
- `.obmem.toml` `vault` must be a relative path inside the repo (no `..`, `~` or absolute paths).
- Repo config can only make secret scanning stricter.
  - `secrets.env_files` entries are added to the defaults, never replace them, and must be relative paths inside the repo. Symlinks that leave the repo are skipped.
  - `scan_process_env = false` from the repo is ignored. To turn it off, set it in your own `~/.config/obmem/config.toml` under `[secrets]`.
- Files are written through a randomly named temp file that is created exclusively, then renamed. A file or symlink planted in the vault cannot redirect the write.
- `obmem init` never writes through a symlink, including a dangling one, and checks that each file really lands inside its repo or vault.
- Every write checks that its real (symlink-resolved) folder is still inside its vault, and refuses to write through a symlinked file.
- `name` is reduced to a single path component before it is used inside the global vault.
- Finding notes can be changed only with `memory_resolve` / `obmem resolve`, not with `memory_write`.
- `obmem audit --write` resets any status outside the finding lifecycle (for example `draft`) back to `open`.

## Roadmap

- ~~P1 `memory_audit`: broken links, orphans, schema, stale notes, findings Base view, suppression list~~ (done)
- ~~P2 code drift: compare `code_refs` + `verified_commit` against `git log`~~ (done)
- P3 semantic audit: embeddings, contradiction detection, decision vs. code consistency

## License

[MIT](LICENSE) © 2026 Chia1104
