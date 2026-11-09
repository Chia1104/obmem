# obmem

Long-term memory for coding agents: an MCP server, agent hooks and a CLI in one Rust binary, storing Obsidian notes that people review. README.md describes what users see. This file and each crate's `AGENTS.md` describe how the code works.

## Engineering principles

- Do not preserve backward compatibility. Remove obsolete paths instead of adding compatibility layers, fallbacks, or migrations.
- Choose the simplest implementation that fully meets the current requirements. Avoid speculative abstractions, configuration, and indirection.
- Grow the system in layers. Start from the smallest version that works end to end, and add each new capability on top of a product that already works. Never trade a working product for unfinished complexity.
- Keep components modular and concerns clearly separated.
- Prefer established, well-maintained libraries when they reduce overall complexity or improve reliability. Do not reimplement common functionality without a clear reason.
- Lean on the dependencies already in the project before writing your own implementation or adding packages. Do not assume a library lacks a capability without checking its documentation and types.
- Make architectural decisions for the long term. Do not accept a stopgap that only works for now and is meant to be replaced later.

## Workspace

| Crate | Owns | Read before changing it |
| --- | --- | --- |
| `crates/obmem-core` | The memory model: config, vaults, notes, search, context ranking, audit, code drift, secret scanning, `init`, `import` | `crates/obmem-core/AGENTS.md` |
| `crates/obmem-mcp` | The MCP server and how it finds the project | `crates/obmem-mcp/AGENTS.md` |
| `crates/obmem-agents` | Hook entry points and `obmem setup` for each agent | `crates/obmem-agents/AGENTS.md` |
| `crates/obmem` | The `obmem` binary: clap CLI | `crates/obmem/AGENTS.md` |
| `apps/obmem-gui` | The desktop control panel (Native Shell, macOS and Windows) | `apps/obmem-gui/AGENTS.md` |

Dependencies flow one way: `obmem` and `obmem-gui` → `obmem-mcp` / `obmem-agents` → `obmem-core`. Put logic in the lowest crate that needs it, so every front end can reuse it. `default-members` leaves out the GUI, so a plain `cargo build` stays free of Native Shell. `--workspace` includes it.

## Workflow

- Before committing: `cargo test --workspace` and `cargo clippy --workspace --all-targets` both pass with no warnings.
- This repo dogfoods obmem. `.mcp.json`, the agent hooks and `.githooks/pre-commit` (enabled with `git config core.hooksPath .githooks`) all run `obmem` from `PATH`. After a change you want the hooks to pick up, run `cargo install --path crates/obmem`.
- Commit messages use Conventional Commits (`feat:`, `fix:`, `refactor:`, `docs:`, `style:`, `chore:`).
- Write token-shaped test fixtures in pieces, for example `concat!("gh", "p_…")`. GitHub push protection rejects a push when any of its commits contains a literal that looks like a token, even a fake one.

<!-- obmem:guide -->
## Project memory (obmem)

obmem is long-term memory for this project (repo vault) and this user (personal vault), stored as Obsidian notes that humans review.

Before a non-trivial task: memory_context with the task description. For precise lookups: memory_search when the task touches something a past session may have decided, learned or been told (Obsidian search syntax: words are ANDed, use OR for alternatives, "phrases", tag:, path:). Prefer what memory says over re-deriving it, but treat `draft` notes as unreviewed.

Record with memory_write, one idea per note, linking related notes with [[note-id]]:
- decision: what was chosen and why, alternatives considered, consequences
- fact / architecture: non-obvious behaviour of the system, with code_refs (path#symbol)
- procedure: steps that took effort to figure out
- feedback / preference: how the user wants agents to work (goes to the personal vault)
Do not record what the code or git history already says, one-off task details, or secrets.

Before finishing: memory_log 1-3 lines (what was done, what is left, what the next session must know).
After reading the code a note describes and finding it still true: memory_verify.
Human-authored notes are read-only: write a finding instead. Findings: memory_resolve (fixed/accepted).
If a tool says "no obmem project here", repeat the call with `project` set to the absolute path of your workspace root; the server remembers it for the rest of the session.
<!-- /obmem:guide -->
