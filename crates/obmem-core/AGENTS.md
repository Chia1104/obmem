# obmem-core

The memory model every front end shares: config, the two vaults, notes, search, context ranking, audit, code drift, secret scanning, `init` and `import`.

## Boundaries

- This crate has no transport and no UI, so there is no rmcp, tokio or terminal output here. An enum that the CLI takes as an argument derives clap only behind the `clap` feature: `#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]`.
- `schemars::JsonSchema` derives become the MCP tool schemas. The doc comments on those enums and their variants are the descriptions agents read.
- `audit` and `store` call each other, so they belong in the same crate.
- `projects` is the machine's list of project roots (`~/.config/obmem/projects.toml`, next to the global config). Front ends record into it: the CLI's `init` and session-start hook, and the GUI. Library functions never record, because their tests would fill the user's real list with temp folders.

## Obsidian (`obsidian.rs`)

obmem keeps no index of its own. Search, backlinks, unresolved links and orphans all come from the Obsidian CLI.
- Every call passes `vault=<id>`. The id is looked up by path in Obsidian's `obsidian.json`, because from a folder that is not a registered vault the CLI silently uses whichever vault is active.
- When the app, the CLI or a vault is unavailable, skip the affected part and say what was skipped. Writes, the session-start context and the file-based audit rules read files directly and keep working.
- Before changing how obmem calls the CLI, run memory_search `obsidian cli` for the quirks found so far.

## Writes (`store.rs`)

`Store::write` is the only way a note changes. Each write goes through these checks:
- Refuse near-duplicate titles (trigram similarity ≥ 0.5) and point at `update_id`.
- Human-authored notes (`author` not `agent:*`) are read-only for agents.
- New notes start as `draft`. An agent edit of an `active` note sets it back to `draft`. `superseded` and `archived` notes cannot be edited.
- Findings change only through resolve, never through write.
- Only a person approves a draft (`Store::approve`: `draft` → `active`). The CLI and GUI offer it; the MCP server does not.
- `code_refs` record `verified_commit` = HEAD.
- Every persisted field is secret-scanned.

## Security invariants

The project vault and `.obmem.toml` may come from an untrusted clone. Input the repo controls can only make obmem stricter:
- Vault rule files add rules but cannot override built-in rule ids. Allowlist files cannot add rules.
- Project-vault allowlists contribute exact fingerprints only. `regexes` and `stopwords` count only in the personal global vault, and even there, stopwords shorter than 4 characters and regexes that match almost anything are ignored.
- In `.obmem.toml`, `vault` must be a relative path inside the repo. `secrets.env_files` entries are added to the defaults, never replace them, and must be relative paths inside the repo; symlinks that leave the repo are skipped. `scan_process_env = false` from the repo is ignored.
- Files are written to an exclusively created, randomly named temp file, then renamed into place. The real (symlink-resolved) folder must stay inside its vault, and obmem never writes through a symlinked file. `init` does not write through any symlink, dangling ones included. A global-vault `name` is reduced to one path component.

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
