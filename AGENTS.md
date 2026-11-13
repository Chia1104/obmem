# obmem

Long-term memory for coding agents: an MCP server, agent hooks and a CLI in one Rust binary, storing Obsidian notes that people review. A project's notes are in a vault the team shares, in the repo or in a private folder outside it; a person's own are in their personal vault. README.md describes what users see. This file and each crate's `AGENTS.md` describe how the code works.

## Engineering principles

- Do not preserve backward compatibility. Remove obsolete paths instead of adding compatibility layers, fallbacks, or migrations.
- Choose the simplest implementation that fully meets the current requirements. Avoid speculative abstractions, configuration, and indirection.
- Grow the system in layers. Start from the smallest version that works end to end, and add each new capability on top of a product that already works. Never trade a working product for unfinished complexity.
- Keep components modular and concerns clearly separated.
- Lean on the dependencies already in the project first, then on established, well-maintained libraries, before writing your own implementation. Do not assume a library lacks a capability without checking its documentation and types.
- Make architectural decisions for the long term. Do not accept a stopgap that only works for now and is meant to be replaced later.

## Workspace

| Crate | Owns | Read before changing it |
| --- | --- | --- |
| `crates/obmem-core` | The memory model: config, vaults, notes, search, context ranking, audit, code drift, secret scanning, `init`, `import` | `crates/obmem-core/AGENTS.md` |
| `crates/obmem-mcp` | The MCP server and how it finds the project | `crates/obmem-mcp/AGENTS.md` |
| `crates/obmem-agents` | Hook entry points and `obmem setup` for each agent | `crates/obmem-agents/AGENTS.md` |
| `crates/obmem-sync` | Encrypted sync and backup of the personal vault through an S3-compatible bucket | `crates/obmem-sync/AGENTS.md` |
| `crates/obmem-update` | How obmem was installed, the newest version, and updating a standalone install | `crates/obmem-update/AGENTS.md` |
| `crates/obmem` | The `obmem` binary: clap CLI | `crates/obmem/AGENTS.md` |
| `apps/obmem-gui` | The desktop control panel (Native Shell, macOS and Windows) | `apps/obmem-gui/AGENTS.md` |

Dependencies flow one way: `obmem` and `obmem-gui` → `obmem-mcp` / `obmem-agents` / `obmem-sync` / `obmem-update` → `obmem-core`. Put logic in the lowest crate that needs it, so every front end can reuse it.

What a library reports to a person about work that went through is a typed value, never a `String`: `settings::Rule`, `secrets::Ignored`, `store::Decided` and `import::Skip` in obmem-core, `setup::ManualStep` in obmem-agents, `Outcome`, `Summary` and `Cleanup` in obmem-sync. Its `Display` is the English the CLI and the MCP server print. The desktop app says the same value in the language its window is in (`apps/obmem-gui/src/words.rs`), so a new one needs a case there and its keys in `locales/app.yml`. An error stays an `anyhow` error with English text.

## Workflow

- Before committing: `cargo fmt --all`, then `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` both pass. CI (`.github/workflows/ci.yml`) runs the same three checks on every push to `main` and every pull request, and the pre-commit hook refuses staged Rust files that are not formatted.
- `default-members` leaves out the GUI, so a plain `cargo build` stays free of Native Shell; `--workspace` includes it. CI also lints and tests the default members on Linux, where remote agents run obmem; the GUI does not build there.
- Releasing a version, or changing the release workflow, the npm packages or the install scripts: load the `release` skill (`.agents/skills/`).
- Commits that only reformat go in `.git-blame-ignore-revs`. Run `git config blame.ignoreRevsFile .git-blame-ignore-revs` once so local `git blame` skips them.
- This repo dogfoods obmem. `.mcp.json`, the agent hooks and `.githooks/pre-commit` (enabled with `git config core.hooksPath .githooks`) all run `obmem` from `PATH`. After a change you want the hooks to pick up, run `cargo install --path crates/obmem`. After a change to a skill in `crates/obmem-agents/skills/`, also run `obmem setup claude codex cursor amp` and commit the copies it rewrites.
- Open work is tracked in the Linear project Obmem. The `linear-workflow` skill (`.agents/skills/`) has the rules: load it before you create, change or close an issue, and when you open a pull request for one.
- Commit messages use Conventional Commits (`feat:`, `fix:`, `refactor:`, `docs:`, `style:`, `chore:`).
- Write token-shaped test fixtures in pieces, for example `concat!("gh", "p_…")`. GitHub push protection rejects a push when any of its commits contains a literal that looks like a token, even a fake one.
