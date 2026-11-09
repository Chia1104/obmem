# obmem-core

The memory model every front end shares: config, the two vaults, notes, search, context ranking, audit, code drift, secret scanning, `init` and `import`.

## Boundaries

- This crate has no transport and no UI, so there is no rmcp, tokio or terminal output here. An enum that the CLI takes as an argument derives clap only behind the `clap` feature: `#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]`.
- `schemars::JsonSchema` derives become the MCP tool schemas. The doc comments on those enums and their variants are the descriptions agents read.
- `audit` and `store` call each other, so they belong in the same crate.
- `projects` is the machine's list of project roots (`~/.config/obmem/projects.toml`, next to the global config). Front ends record into it: the CLI's `init` and session-start hook, and the GUI. Library functions never record, because their tests would fill the user's real list with temp folders.

## Settings and allowlists (`settings.rs`, `allowlist.rs`)

- These edit the config files and the vaults' `_system/secret-allowlist.toml` the way a person does, through `toml_edit`: the rest of the file stays, comments included.
- A save is checked by the same functions `Config::load` and the scanner use (`project_vault_path`, `is_plain_relative`, `secrets::regex_problem` / `stopword_problem`). Put a new rule there, not in the editor, so that what can be saved is exactly what loads.

## Review (`review.rs`)

`Store::review` lists what waits for a person: drafts (except findings and session logs), and findings that are `open` or `accepted`. An audit finding's `target` is the vault-relative path of the note it is about, in the same vault.

`Store::approve`, `resolve_finding` and `verify` take a `NoteRef`. A `Key` (id, title or path, in either vault) is for names people type. When the name comes from a note's own content, such as a finding's `target`, use `At { scope, rel }`: it matches only that exact path in that vault. A planted finding in a cloned project vault could otherwise aim the action at a note in the personal vault.

## Session logs and import (`sessions.rs`, `import.rs`)

- `sessions::logs` reads a project's session logs, newest first, and `sessions::entries` splits a log into entries. The session-start context uses the same parser.
- `import::target` is where an imported memory would go. A file already there means it is skipped, so a front end can show that before importing.

## Obsidian (`obsidian.rs`)

obmem keeps no index of its own. Search, backlinks, unresolved links and orphans all come from the Obsidian CLI.
- Every call passes `vault=<id>`. The id is looked up by path in Obsidian's `obsidian.json`, because from a folder that is not a registered vault the CLI silently uses whichever vault is active.
- When the app, the CLI or a vault is unavailable, skip the affected part and say what was skipped. Writes, the session-start context and the file-based audit rules read files directly and keep working.
- Before changing how obmem calls the CLI, run memory_search `obsidian cli` for the quirks found so far.
- `open_uri` builds the `obsidian://open?vault=<id>&file=<path>` link that front ends use to open a note.

## Writes (`store.rs`)

`Store::write` is the only way a note changes. Each write goes through these checks:
- Refuse near-duplicate titles (trigram similarity ≥ 0.5) and point at `update_id`.
- Human-authored notes (`author` not `agent:*`) are read-only for agents.
- New notes start as `draft`. An agent edit of an `active` note sets it back to `draft`. `superseded` and `archived` notes cannot be edited.
- Findings change only through resolve, never through write.
- Only a person approves a draft (`Store::approve`: `draft` → `active`). The CLI and GUI offer it; the MCP server does not.
- The exception is the user's own choice: with `[review] auto_approve = true` in their config, `write` approves new notes and edits itself (`approved_by: auto-approve`). Findings keep their own lifecycle.
- `code_refs` record `verified_commit` = HEAD.
- Every persisted field is secret-scanned.

## Security invariants

The project vault and `.obmem.toml` may come from an untrusted clone. Input the repo controls can only make obmem stricter:
- Vault rule files add rules but cannot override built-in rule ids. Allowlist files cannot add rules.
- Project-vault allowlists contribute exact fingerprints only. `regexes` and `stopwords` count only in the personal global vault, and even there, stopwords shorter than 4 characters and regexes that match almost anything are ignored.
- `[review] auto_approve` is on only when the user's config turns it on. The repo's `.obmem.toml` can turn it off for that project, never on.
- In `.obmem.toml`, `vault` must be a relative path inside the repo. `secrets.env_files` entries are added to the defaults, never replace them, and must be relative paths inside the repo; symlinks that leave the repo are skipped. `scan_process_env = false` from the repo is ignored.
- Files are written to an exclusively created, randomly named temp file, then renamed into place. The real (symlink-resolved) folder must stay inside its vault, and obmem never writes through a symlinked file. `init` does not write through any symlink, dangling ones included. A global-vault `name` is reduced to one path component.
- Projects with the same `name` share `Projects/<name>/Sessions`. A session log's header records the real project root, compared as text and never resolved. `Store::log` adds only to a log that records this project's root, else starts `<log>-2.md`, and `sessions::logs` leaves out logs that record another root.
- A session log's name holds the device (`<date>-<device>[-<session>]`, `Config::device` from `config::device_name`). The personal vault is synced between machines, and two machines writing one file would conflict every day.

Any change on these paths keeps an existing test for the invariant, or adds one.

## memory_context ranking (`context.rs`)

Terms are cut by Unicode script, not by language:
- Scripts written with spaces: words of 3 or more characters, with combining marks kept.
- Han: bigrams. Katakana: runs kept whole. Hiragana: dropped.
- Hangul, and Thai / Lao / Khmer / Myanmar: bigrams of character clusters.
- Small English and Chinese filler lists drop common words. With more than 16 terms, the longest win.

Each note is ranked by how many distinct terms it contains, with title matches counting double. Matching is literal: there is no stemming and no matching across languages.

## Audit and code drift

- A finding names its note by path, never with a `[[link]]`. Obsidian counts a link as a backlink, so a linked note would stop being an orphan.
- When a human sets a finding to `rejected`, its fingerprint goes into `_system/audit-suppressions.toml` for good. Agents cannot reject.
- A code-drift fingerprint includes the commit. After `verify`, the next `audit --write` closes the old finding.
- `sync-conflict` reports files whose name holds `SYNC_CONFLICT_MARK`, the copies obmem-sync writes when two devices changed the same file. Every other rule ignores those copies, so a conflict is reported once.
