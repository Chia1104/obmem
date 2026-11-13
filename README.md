# obmem

![obmem — Long-term memory for coding agents](.github/public/obmem-og.png)

Long-term memory for coding agents, stored as Obsidian notes that people review.

obmem is one Rust binary that works as an MCP server, a set of agent hooks and a CLI. Agents read and write plain Markdown notes, and people browse and review them in Obsidian. Search, backlinks and link checks come from Obsidian itself through its [official CLI](https://obsidian.md/help/cli), so agents see the same results as the app. On a machine without Obsidian, such as a server or a cloud sandbox, obmem reads the vault files itself.

## Two vaults

| Vault | Default location | Holds |
|---|---|---|
| project | `<repo>/docs/kb`, committed and shared with the team | decisions, facts, concepts, architecture, procedures, findings |
| global | `~/Obsidian/AgentMemory`, personal | feedback and preferences, session logs |

### A private project vault

A team that does not want its memory in the repo keeps the project vault in a folder outside it:

```sh
obmem init --private ~/Obsidian/acme-memory
```

- Your own config records the folder, under `[[project]]` in `~/.config/obmem/config.toml`. `.obmem.toml` only says `private = true`, so the repo never says where the notes are.
- Each person runs the command once on each machine, with the folder that holds their copy of the vault. Until then obmem refuses the project there, and writes no note into the repo.
- obmem does not carry a private vault between machines yet: share the folder your own way, for example as a private git repository. `obmem sync` covers the personal vault only.
- On a project that already has `docs/kb`, the command switches it to the private vault and leaves the old notes where they are. Move them yourself, and take the folder out of git.

## Requirements

- Obsidian 1.12 or later, with Settings → General → **Command line interface** turned on.
- Open each vault folder once with "Open folder as vault". Obsidian cannot register a vault from the CLI. `obmem status` shows which vaults are registered.

If Obsidian is closed, obmem starts it in the background. If the app or a vault is unavailable, obmem skips search, backlinks and link checks for that vault, says so in its output, and everything else keeps working.

### Without Obsidian

When no `obsidian` command is on `PATH`, obmem needs nothing from the list above. It reads the vault files itself for search, backlinks and link checks, and `obmem status` shows `search: built in`. The choice is made when obmem starts: on a machine that has the CLI, obmem never falls back to its own search.

The built-in search differs from Obsidian in these ways:
- It supports words, `"phrases"`, `OR`, `-exclusion`, `( )`, `file:`, `path:`, `content:`, `tag:`, `line:( )`, `[property]` and `[property:value]`. It refuses regular expressions and `block:`, `section:`, `task:`, `match-case:` and `ignore-case:` with an error.
- Only `[[wikilinks]]` count as links. A note linked only with a Markdown link (`[text](note.md)`) is reported as an orphan.

## Install

```sh
npm install -g @chia1104/obmem   # a prebuilt binary for macOS, Linux or Windows
cd your-repo
obmem init                  # create both vaults and .obmem.toml
obmem setup claude codex    # register the MCP server and hooks for your agents
obmem status
```

Install it globally: the MCP config and the hooks that `setup` writes run `obmem` from `PATH`, in every agent turn. `npx @chia1104/obmem <command>` works for a command you run once.

Without Node, the install script of a release puts the same binary in `~/.obmem/bin`, after checking it against the release's `SHA256SUMS`:

```sh
curl -fsSL https://github.com/Chia1104/obmem/releases/latest/download/install.sh | sh
```

```powershell
irm https://github.com/Chia1104/obmem/releases/latest/download/install.ps1 | iex
```

Set `OBMEM_VERSION` (such as `0.1.0`) for another release than the latest, and `OBMEM_INSTALL_DIR` for another folder. The script changes nothing else: when the folder is not on `PATH`, it says what to add.

### Updating

```sh
obmem update            # update to the newest release
obmem update --check    # only say whether there is one
```

`obmem update` replaces a binary that the install script put there, using the new release's install script and its `SHA256SUMS`. A binary that npm or cargo installed is theirs to update, so there it prints the command to run, such as `npm install -g @chia1104/obmem@latest`. obmem never checks for updates on its own.

With more than one install on a machine, the first `obmem` on `PATH` is the one that runs. `obmem status` shows which binary that is and how it was installed.

Other ways to get the binary:
- Download it from the [releases](https://github.com/Chia1104/obmem/releases): macOS (Apple silicon and Intel), Linux (x64 and arm64, static) and Windows (x64).
- Build it from a clone: `cargo install --path crates/obmem`.

## Agents

`obmem setup <agent>...` writes the agent's project config and installs obmem's skills. It merges into existing config files, so running it again is safe. The hooks do nothing in a project without `.obmem.toml`.

| Agent | Config `setup` writes | Skills go to |
|---|---|---|
| Claude Code (`claude`) | `.mcp.json`, `.claude/settings.json` | `.claude/skills/` |
| Codex (`codex`) | `.codex/config.toml`, `.codex/hooks.json` | `.agents/skills/` |
| Cursor (`cursor`) | `.cursor/mcp.json`, `.cursor/hooks.json` | `.agents/skills/` |
| GitHub Copilot CLI (`copilot`) | `.github/hooks/obmem.json`; add the MCP server to `~/.copilot/mcp-config.json` yourself | `.agents/skills/` |
| Amp (`amp`) | `.amp/settings.json`, `.amp/plugins/obmem.ts`; approve the MCP server with `amp mcp approve obmem` | `.agents/skills/` |

The skills tell the agent how to use the memory, and it loads one when it needs it:

| Skill | For |
|---|---|
| `obmem` | searching memory, and what to record as a note |
| `obmem-refresh` | bringing notes back in line with the code after it changed |

The skill files are obmem's. `setup` rewrites them when a new version of obmem brings a new text, so put a project's own guidance in a skill of its own. Commit them with the rest of the config.

Once set up:
- At session start, the agent gets the vault index, where the last session left off, the feedback and preferences you approved, open findings, and unreviewed drafts.
- When a session ends after real work that has not been logged, the agent is asked once to log it.
- Before the context is compacted, obmem writes a session-log entry.

## MCP tools

| Tool | Does |
|---|---|
| `memory_context` | The notes most relevant to a task description, in any language |
| `memory_search` | Obsidian search across both vaults, with the same syntax as the app's search box |
| `memory_get` | One note, with its backlinks |
| `memory_write` | Create or update a note. New notes start as `draft` (or `active`, with [auto-approve](#auto-approve)), notes written by people are read-only for agents, and near-duplicates and secrets are refused |
| `memory_log` | Append to today's session log |
| `memory_audit` | Run the audit, optionally writing finding notes |
| `memory_resolve` | Mark a finding `fixed`, `accepted` or `open` |
| `memory_verify` | Record that a note still matches the code |

The CLI has the same features, plus what only people may do: `obmem approve <note>` accepts a draft, and `obmem resolve <finding> rejected` silences a false positive. Run `obmem --help` to list the commands.

## Auto-approve

Every note an agent writes waits as a `draft` until you approve it. To skip that review, turn on auto-approve in your own config, or in the desktop app's settings, under Personal vault:

```toml
# ~/.config/obmem/config.toml
[review]
auto_approve = true
```

Agent notes are then written `active`, with `approved_by: auto-approve`, and so are agent edits and Claude Code imports. Findings still start `open`. A project's `.obmem.toml` can turn auto-approve off for that project with the same `[review]` setting set to `false`, but it cannot turn it on: a cloned repo must not decide that what agents write skips your review.

## Audit

`obmem audit` checks both vaults with fixed rules. It does not call an LLM.

| Rule | Catches |
|---|---|
| `broken-link` | Links to notes that do not exist |
| `schema` | Invalid frontmatter, types, statuses and dates |
| `stale` | Active notes not verified for 180 days |
| `supersede` | Links to superseded notes |
| `duplicate` | Notes with near-identical titles |
| `orphan` | Notes nothing links to |
| `code-drift` | `code_refs` whose file or symbol is gone, or whose code changed since the note was verified |
| `sync-conflict` | Conflict copies that [sync](#sync-and-backup) left next to a note |

```sh
obmem audit                        # report warnings and errors
obmem audit --write                # create, close and reopen finding notes
obmem resolve <finding> rejected   # a false positive: never report it again
obmem verify <note>                # the note was re-checked against the current code
```

Review findings in Obsidian with `_system/Findings.base`. To tune or disable rules, use `[audit]` in `.obmem.toml` and `_system/audit-suppressions.toml`.

A note anchors itself to code with `code_refs`: `path/to/file.rs#symbol`, `path/to/file.rs#Type::method`, `path/to/file.rs:42`, or a directory `path/to/dir/`.

## Secret scanning

Every write is scanned for secrets in three ways:
- the real values in `.env*` files and in sensitive-looking environment variables
- format rules for known token prefixes ([gitleaks format](crates/obmem-core/src/secret_rules.toml); add your own in `_system/secret-rules.toml`)
- `password = …`-style assignments whose value looks random

By default a write that contains a secret is refused. Set `mode = "redact"` to write `[REDACTED:rule]` instead. To allow a false positive, add the fingerprint from the error to `_system/secret-allowlist.toml`. A fingerprint allows that one value: for a private key it covers the whole key. Config and rules in a cloned repo can only make scanning stricter.

A value in an `.env*` file that starts with `./`, `../` or `~/` is taken for a path and left out. Any other value there counts, absolute paths included, because obmem never looks a value from those files up on disk.

For human edits, use `obmem scan --staged` as a git pre-commit hook. It checks the staged files in the project vault.

## Sync and backup

obmem can sync the personal vault between your machines through an S3-compatible bucket you own: AWS S3, Cloudflare R2 or MinIO. Every sync is also a backup snapshot. Everything in the bucket is encrypted with a key that only your passphrase unlocks, file names included. macOS and Windows machines can share one bucket.

```toml
# ~/.config/obmem/config.toml
[sync]
endpoint = "https://<account-id>.r2.cloudflarestorage.com"  # leave out for AWS S3
bucket = "my-obmem-vault"
# region = "us-east-1"   # AWS needs the bucket's region; R2 uses "auto", the default
# prefix = "obmem"       # key prefix inside the bucket
# device = "work-mac"    # this machine's name; `setup` sets it to the host name plus 4 random hex digits
# community_plugins = false
# auto = true            # sync in the background as agent sessions start and turns end
# ephemeral = false      # true on a machine thrown away after a session (see below)
```

```sh
obmem sync setup                  # access key, then choose (first device) or enter the passphrase
obmem sync                        # sync now
obmem sync list                   # snapshots: number, time, device, file count
obmem sync restore --snapshot 12 --to ~/restored   # an older state, into an empty folder
obmem sync prune --dry-run        # what retention would remove (it runs by itself daily)
```

Once a device is set up, obmem syncs on its own: when an agent session starts in an obmem project, and when an agent turn ends, the hook starts `obmem sync` in the background, so the agent never waits on the network. Such syncs start at most once a minute; the last one always runs. `obmem status` and the desktop app's settings, under Sync, show the last sync and why it failed, if it did. Set `auto = false` to sync only by hand. The desktop app can also edit `[sync]`, connect the device and sync now.

`setup` keeps the access key and the unlocked vault key in the OS keychain (macOS Keychain, Windows Credential Manager), never in a config file. **If you forget the passphrase, the bucket cannot be read.**

On a new machine, run `obmem sync setup` and then `obmem sync`. What the machine already has is merged in, not replaced.

### On a server or in a cloud sandbox

A machine without a keychain takes its credentials from the environment instead of `obmem sync setup`:

| Variable | Value |
|---|---|
| `OBMEM_SYNC_ACCESS_KEY_ID` | the bucket's access key id |
| `OBMEM_SYNC_SECRET_ACCESS_KEY` | the bucket's secret access key |
| `OBMEM_SYNC_VAULT_KEY` | the backup key. `obmem sync key` prints it on a machine that is already set up |

Set all three: when any of them is set, obmem does not ask the keychain. Keep them in the platform's secret store. The backup key reads your whole personal vault, and what an agent writes on that machine syncs to all your machines.

For a machine that is thrown away after a session, such as a cloud agent's sandbox, install obmem (`npm install -g @chia1104/obmem`) and write a config with `ephemeral = true` in its setup script:

```toml
# ~/.config/obmem/config.toml
vault = "~/AgentMemory"

[sync]
endpoint = "https://<account-id>.r2.cloudflarestorage.com"
bucket = "my-obmem-vault"
ephemeral = true
```

On an ephemeral machine:
- The session-start hook syncs before it reads the memory context, and waits for it. The first sync creates the vault folder and downloads the vault.
- The hook that ends a turn waits for its sync too, because the machine may be gone before a background sync finishes.
- The machine never removes old snapshots.

Leave `device` unset on sandboxes that can run at the same time. Each one then writes its own session log. With one shared name, two sandboxes write the same log, and the sync turns one of them into a conflict copy.

How a sync decides:
- A file changed on one side only: that version wins. Deletions travel the same way.
- A file changed on both sides: your version stays, and the other machine's version lands next to it as `<name>.sync-conflict-<device>-<time>.md`. `obmem audit` reports it (rule `sync-conflict`) until you merge what you need and delete the copy.
- A file deleted on one side and edited on the other: the edit is kept.
- A sync that would delete more than half of the vault stops and asks for `--allow-deletes`.

Session logs carry the machine's name (`2026-11-09-work-mac.md`), so two machines never write the same log.

A sync covers the notes, `_system/`, and Obsidian's settings, themes and snippets from `.obsidian/`. It leaves out the workspace layout, other dot-folders and symlinks, plus community plugins unless `community_plugins = true`. A file whose name Windows cannot hold (`:` or `?`, a name like `con`, a trailing dot) is skipped with a warning.

Older snapshots stay in the bucket as restore points: every snapshot from the last 7 days, the newest of each day for 30 days, the newest of each week for a year, and always the latest. Once a day, after a sync, obmem removes the rest and the content only they needed. Run `obmem sync prune --dry-run` to see what would go.

### In an Amp orb

An [orb](https://ampcode.com/docs/orbs) is Amp's cloud sandbox. It has the repository and nothing from your own machine, so three things must reach it:

- **The agent config.** Commit the files `obmem setup amp` writes. The first thread in an orb asks whether to run the `obmem` MCP server: choose **Always Allow for Me**.
- **The credentials.** Add the three `OBMEM_SYNC_*` variables as secrets in your personal Amp settings, so that only your orbs get them.
- **obmem and its config.** Two executable scripts in the repository do it:

```bash
#!/usr/bin/env bash
# .agents/setup: runs when Amp prepares the orb
set -euo pipefail
command -v obmem >/dev/null 2>&1 || npm install -g @chia1104/obmem
```

```bash
#!/usr/bin/env bash
# .agents/resume: runs when the orb starts and each time it wakes
set -euo pipefail
[ -n "${OBMEM_SYNC_VAULT_KEY:-}" ] || exit 0 # no backup key: nothing to do
mkdir -p ~/.config/obmem
cat > ~/.config/obmem/config.toml <<'EOF'
vault = "~/AgentMemory"

[sync]
endpoint = "https://<account-id>.r2.cloudflarestorage.com"
bucket = "my-obmem-vault"
ephemeral = true
EOF
obmem sync
```

Never sync in `.agents/setup`. Amp saves what that script leaves behind as a snapshot, and the orbs of everyone in the project start from it. It also runs without your personal secrets. `.agents/resume` runs with them.

Amp waits ten seconds for `.agents/resume` and then lets it finish in the background. When a large vault is still downloading at the first turn, the session-start hook waits for that sync, for up to two minutes.

In a repository you share, the bucket is yours and not the team's: take `endpoint` and `bucket` from personal environment variables of your own instead of writing them in the script.

## Import Claude Code memory

`obmem import-claude` imports `~/.claude/projects/<project>/memory/*.md` as draft notes (active with auto-approve). User and feedback memories go to the global vault, project and reference memories to the project vault. Files that already exist are skipped.

## Desktop app

`apps/obmem-gui` is a control panel for macOS and Windows:
- **Projects:** every project obmem has seen (each agent session records its project), and adding one by hand.
- **Overview:** each project's health, setting up agents with one click, and importing the project's Claude Code memory (pick which).
- **Agent view:** what the session-start hook gives an agent, and what `memory_context` returns for a task you type.
- **Sessions:** the project's session logs, newest first.
- **Settings:** `.obmem.toml`, and whether Codex runs obmem's tools without asking.
- **Secrets:** try the scanner on any text and allow a false positive in one click.
- **Review:** agent drafts and open findings across projects. Approve a draft, mark a finding fixed, accepted or rejected, verify the note it is about, open any of them in Obsidian, and run the audit.
- **Settings**, for every project, in four pages:
  - **Preferences:** the language, and how the window looks: light, dark or the system's, and a theme for each. Thirteen themes are built in. Your own are JSON files in `~/.config/obmem/themes/`, in shadcn/ui's theme format (what [tweakcn](https://tweakcn.com) exports): duplicate a built-in one where you pick a theme and edit the file. `~/.config/obmem/appearance.toml` keeps what you chose. The app speaks English or Traditional Chinese: it follows the system's language and shows English for any other, and a language picked here stays whatever the system's. The reason an action failed stays in English, as the CLI reports it.
  - **Personal vault:** where it is, environment-variable scanning, and auto-approve.
  - **Sync:** the bucket, connecting this device, the last sync, and syncing now.
  - **Allowed secrets:** the fingerprints, patterns and words the scanner lets pass in every project.

```sh
cargo run -p obmem-gui
```

## Development

A Cargo workspace:

| Crate | Holds |
|---|---|
| `crates/obmem-core` | Config, vaults, notes, search, audit, secret scanning |
| `crates/obmem-mcp` | The MCP server |
| `crates/obmem-agents` | Agent hooks and `obmem setup` |
| `crates/obmem-sync` | Encrypted sync and backup of the personal vault through S3-compatible storage |
| `crates/obmem` | The `obmem` binary |
| `apps/obmem-gui` | The desktop app ([Native Shell](https://nsh.dev)) |

```sh
cargo test --workspace    # includes the desktop app, which needs the latest stable Rust
```

How the code works and the rules for changing it are in [AGENTS.md](AGENTS.md) and each crate's `AGENTS.md`.

## License

[MIT](LICENSE) © 2026 Chia1104
