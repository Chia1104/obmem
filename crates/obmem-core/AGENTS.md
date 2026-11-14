# obmem-core

The memory model every front end shares: config, the two vaults, notes, search, context ranking, audit, code drift, secret scanning, `init` and `import`.

## Boundaries

- This crate has no transport and no UI, so there is no rmcp, tokio or terminal output here. An enum that the CLI takes as an argument derives clap only behind the `clap` feature: `#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]`.
- `schemars::JsonSchema` derives become the MCP tool schemas. The doc comments on those enums and their variants are the descriptions agents read.
- `audit` and `store` call each other, so they belong in the same crate.
- `projects` is the machine's list of project roots (`~/.obmem/projects.toml`, next to the global config). Front ends record into it: the CLI's `init` and session-start hook, and the GUI. Library functions never record, because their tests would fill the user's real list with temp folders.

## Git worktrees (`config.rs`)

A linked git worktree is a second folder of one project, so `Config` has two roots:
- `project_root` is where the files are: `.obmem.toml`, a vault in the repo, the env files, the code that `code_refs` and drift read.
- `main_root` is the project: the repo's main checkout (`config::main_checkout`). What obmem keeps outside the project's folder is kept under it: `[[project]]` and `[project.sync]` in the user's config, `projects.toml`, the root in a session log's header, the default project name, and Claude Code's memory folder for an import. Outside a worktree the two roots are the same.

Rules:
- A function that takes a project's root and reads one of those calls `main_checkout` itself (`config::project_sync`, the private-vault functions in `settings`, `projects::record`). Pass it the root you have.
- `main_checkout` asks the repo, never the folder: a folder is a worktree only when the repo's own `git worktree list` has it. A `.git` file is one line that anyone can write, and a folder that only names a private project's repo must not be given that project's vault.
- A vault in the repo is not mapped to the main checkout's. The worktree has its own copy, whose notes merge with the branch, and reads and writes stay in it (see "Without Obsidian" for how it is searched).

## Settings and allowlists (`settings.rs`, `allowlist.rs`)

- These edit the config files and the vaults' `_system/secret-allowlist.toml` the way a person does, through `toml_edit`: the rest of the file stays, comments included.
- A save is checked by the same functions `Config::load` and the scanner use (`project_vault_path`, `is_plain_relative`, `secrets::regex_problem` / `stopword_problem`). Put a new rule there, not in the editor, so that what can be saved is exactly what loads.
- `settings::make_private` is how a project gets a private vault: it adds the project to `[[project]]` in the user's config, with `[project.sync]` when it is given a bucket, and marks `.obmem.toml` `private`. A front end calls it before `Config::load`, which refuses a private project whose vault the user's config does not name with a `config::NoVaultHere` error, so a front end that can ask for the folder finds that case by its type. `settings::private_vault_rule` gives the rule a folder breaks (`config::Misplaced`, as a `Rule`), for a form to show at its field.
- `settings::read_project_sync` and `save_project_sync` are `[project.sync]` as a settings screen takes it: the bucket's four settings in a `SyncSettings`, checked by the rules `[sync]` has. Saving needs the project to have a private vault.
- The rules are also there to ask: `ProjectSettings::problems`, `GlobalSettings::problems` and `SyncSettings::problems` give every rule the settings break, each with its field, and `allowlist::problem` gives why the scanner would ignore an entry. A settings screen shows these at the fields before it saves; a save refuses the first. A rule is a value (`settings::Rule`, `secrets::Ignored`) whose `Display` is a sentence for the person who typed the value; the desktop app says it in its own language. A rule that needs the file system (the vault must not leave the project through a symlink) is checked only by the save.

## Review (`review.rs`)

`Store::review` lists what waits for a person: drafts (except findings and session logs), and findings that are `open` or `accepted`. An audit finding's `target` is the vault-relative path of the note it is about, in the same vault.

`Store::approve`, `resolve_finding` and `verify` take a `NoteRef`. A `Key` (id, title or path, in either vault) is for names people type. When the name comes from a note's own content, such as a finding's `target`, use `At { scope, rel }`: it matches only that exact path in that vault. A planted finding in a cloned project vault could otherwise aim the action at a note in the personal vault.

## Session logs and import (`sessions.rs`, `import.rs`)

- `sessions::logs` reads a project's session logs, newest first, and `sessions::entries` splits a log into entries. The session-start context uses the same parser.
- Claude Code's memory files are written by an agent, so their frontmatter is not always valid YAML. `import::quote_values` quotes a top-level value that holds `: ` or ` #` before the file is parsed. It is for imported files only: `Note::parse` reads obmem's own notes strictly.
- `import::plan` fails on the first file it cannot read and names that file. Nothing has been written by then.
- `import::standing` is what keeps a memory out, as a `Skip`: it was imported before (the note there names its memory file), another note has its name, or a note of the same type and scope has a similar title (`Store::duplicate_of`, the check `write` makes). A front end asks it to show what an import would do, and `import::run` asks it before each write. It reads the catalog as of the store's last `sync`.
- Claude Code keeps one memory folder per project, so a `feedback` memory is imported for this project only (`Planned::this_project_only`). A `user` memory is not: who the user is does not change with the project.
- An imported note sets `WriteRequest::needs_review`, so it is a draft whatever `[review] auto_approve` says. Nobody wrote it under obmem's rules.

## Obsidian (`obsidian.rs`)

obmem keeps no index of its own. Search, a note's links and backlinks, unresolved links and orphans all come from the Obsidian CLI, or from `headless.rs` on a machine without it (see below).
- Every call passes `vault=<id>`. The id is looked up by path in Obsidian's `obsidian.json`, because from a folder that is not a registered vault the CLI silently uses whichever vault is active.
- When the app, the CLI or a vault is unavailable, skip the affected part and say what was skipped. Writes, the session-start context and the file-based audit rules read files directly and keep working.
- Before changing how obmem calls the CLI, run memory_search `obsidian cli` for the quirks found so far.
- `open_uri` builds the `obsidian://open?vault=<id>&file=<path>` link that front ends use to open a note.

## Without Obsidian (`headless.rs`, `query.rs`, `links.rs`)

`obsidian::Api` has two implementations. `Backend::detect` picks one when a store opens: `Cli` when the Obsidian CLI command (`OBMEM_OBSIDIAN_BIN`, else `obsidian`) is on `PATH`, else `Headless`.
- The choice holds for the life of the store. A failed CLI call never falls back to `Headless`: the two differ in details, and on a machine with Obsidian a problem with it must be reported.
- Each vault of a store has its own backend, and they differ in one case (`Backend::for_project_vault`): in a linked git worktree on a machine with Obsidian, the repo's vault is a copy that Obsidian has not opened, and `Headless` reads it. This follows from where the vault is, when the store opens. It is not a fallback: a copy that Obsidian knows is asked there, and so are the main checkout's vault and a private vault.
- `Headless` reads the vault on every call, like the catalog. Do not add an index.
- `query.rs` is the part of Obsidian's search syntax that `Headless` supports. An operator it lacks is an error, never a word to search for. `Store::search` and `context` build queries from this syntax, so what they send must stay inside it.
- `links.rs` (`Names`) decides which file a `[[link]]` opens. `Headless` and the audit's cross-vault hint share it.
- A test that needs one backend opens the store with `Store::open_with`, because `Store::open` depends on the machine.
- A new `Api` method needs both implementations.

## Writes (`store.rs`)

`Store::write` is the only way a note changes. Each write goes through these checks:
- Refuse near-duplicate titles (trigram similarity ≥ 0.5) and point at `update_id`.
- Human-authored notes (`author` not `agent:*`) are read-only for agents.
- New notes start as `draft`. An agent edit of an `active` note sets it back to `draft`. `superseded` and `archived` notes cannot be edited.
- Findings change only through resolve, never through write.
- Only a person approves a draft (`Store::approve`: `draft` → `active`). The CLI and GUI offer it; the MCP server does not.
- The exception is the user's own choice: with `[review] auto_approve = true` in their config, `write` approves new notes and edits itself (`approved_by: auto-approve`). Findings keep their own lifecycle, and a request with `needs_review` (an import) stays a draft.
- `this_project_only` gives a new note for the global vault the project's tag (`Config::project_tag`). An edit keeps the note's `project/…` tags whatever tags it names, and is refused when it asks for a project the note does not hold in: where a note holds is the user's to change.
- `code_refs` record `verified_commit` = HEAD.
- Every persisted field is secret-scanned.

## Security invariants

The project vault and `.obmem.toml` may come from an untrusted clone. Input the repo controls can only make obmem stricter:
- Vault rule files add rules but cannot override built-in rule ids. Allowlist files cannot add rules.
- Project-vault allowlists contribute exact fingerprints only. `regexes` and `stopwords` count only in the personal global vault, and even there, stopwords shorter than 4 characters and regexes that match almost anything are ignored.
- A fingerprint allows one exact secret. A private key's fingerprint is taken over the key through its END line (`secrets::secret_end`), never over the BEGIN line alone: that line is the same for every key, so its fingerprint is public.
- A value from an env file is only compared as text. The scanner never looks one up on disk: on Windows, looking up `//host/share` sends the user's credentials to that host. Only values from the process environment are checked for being an existing path.
- The session-start context lists only `active` feedback and preferences as the user's guidance. A draft one is listed with the unreviewed drafts.
- A note in the global vault with `project/<name>` tags is guidance in those projects only (`Catalog::global_guidance`, `Config::project_tag`). The name is the repo's, so a clone that takes another project's name is shown that project's guidance. Without the tags every project was shown it: a tag narrows who sees a note and never widens it. An agent's edit cannot remove one.
- `[review] auto_approve` is on only when the user's config turns it on. The repo's `.obmem.toml` can turn it off for that project, never on.
- A project vault outside the repo is named only by the user's config (`[[project]]`, checked by `config::private_vault_path`): a folder of its own, outside the project and apart from the personal vault. `.obmem.toml` can say `private = true`, which names no folder: without an entry in the user's config `Config::load` fails, and nothing is written to `docs/kb` instead. `Config::vault_is_private` tells the two kinds apart by the paths.
- The bucket a private vault syncs through is in the user's config too (`[project.sync]`, read by `config::project_sync`), like `[sync]`: never in `.obmem.toml`.
- A private vault is shared with the team like the one in the repo, so every rule here about the project vault holds for it too.
- In `.obmem.toml`, `vault` must be a relative path inside the repo. `secrets.env_files` entries are added to the defaults, never replace them, and must be relative paths inside the repo; symlinks that leave the repo are skipped. `scan_process_env = false` from the repo is ignored.
- Files are written to an exclusively created, randomly named temp file, then renamed into place. The real (symlink-resolved) folder must stay inside its vault, and obmem never writes through a symlinked file. `init` does not write through any symlink, dangling ones included. A global-vault `name` is reduced to one path component.
- A private project's vault is given to its main checkout and to the worktrees the repo lists, never to a folder whose `.git` file only names the repo (`config::main_checkout`).
- Projects with the same `name` share `Projects/<name>/Sessions`. A session log's header records the real root of the project's main checkout, compared as text and never resolved. `Store::log` adds only to a log that records this project's root, else starts `<log>-2.md`, and `sessions::logs` leaves out logs that record another root.
- A session log's name holds the device (`<date>-<device>[-<session>]`, `Config::device` from `config::device_name`). The personal vault is synced between machines, and two machines writing one file would conflict every day.

Any change on these paths keeps an existing test for the invariant, or adds one.

## memory_context ranking (`context.rs`)

Terms are cut by Unicode script, not by language:
- Scripts written with spaces: words of 3 or more characters, with combining marks kept.
- Han: bigrams. Katakana: runs kept whole. Hiragana: dropped.
- Hangul, and Thai / Lao / Khmer / Myanmar: bigrams of character clusters.
- Small English and Chinese filler lists drop common words. With more than 16 terms, the longest win.

Matching is literal: there is no stemming and no matching across languages. The query is the terms as bare words joined by `OR`, never quoted: Obsidian finds a bare word anywhere, and a quoted one almost only as a whole word, which the built-in search does not copy. The ranking then decides again what a note has (`count`): a word of a script written with spaces counts only where a word starts (`rag` is not in `storage`, `token` is in `tokens`), and a bigram counts anywhere.

Each note is ranked by the terms it has:
- A term is worth more the fewer of the found notes have it (`weight`), so `does` and `not` decide little. There is no list of common words to extend for this.
- A note gets more of a term's weight for having it more often, and a long note needs it more often (`share`, the term-frequency part of BM25). A term in the title gets its weight once more.
- Equal scores come project vault first, then by path, so both backends give one order.
- `Index.md` is never a match: it has most terms of any task. When it has some, the answer names it in one line.

The agent that calls `memory_context` can search again, so the answer (`context::Recall`) also says where to go next:
- Each match lists the terms it contains.
- `unmatched` is the parts of the task that no note contains, as the task wrote them. It is empty when that is unknown: a vault was not searched, or the search stopped at `CANDIDATES`.
- `linked` is the notes one link away, in either direction, from the three best matches. The vault's `Index.md` links to every note, so it is never followed or listed.
- `recall` gives the values and `Recall::render` the text. Put a new signal in the struct first, so the measurement below can read it.

`tests/recall.rs` measures all of this on this repo's `docs/kb`: tasks, and the notes each should find. Run it with `--nocapture` before and after a change to the terms, the ranking or the links, and add a case when a real task finds the wrong notes.

## Audit and code drift

- A finding names its note by path, never with a `[[link]]`. Obsidian counts a link as a backlink, so a linked note would stop being an orphan.
- When a human sets a finding to `rejected`, its fingerprint goes into `_system/audit-suppressions.toml` for good. Agents cannot reject.
- A code-drift fingerprint includes the commit. After `verify`, the next `audit --write` closes the old finding.
- `sync-conflict` reports files whose name holds `SYNC_CONFLICT_MARK`, the copies obmem-sync writes when two devices changed the same file. Every other rule ignores those copies, so a conflict is reported once. The catalog and `Store::search` leave them out too, so agents never take another device's unmerged version for a note.
