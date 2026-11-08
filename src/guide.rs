//! How agents should use the memory. One text, three channels: the MCP server instructions
//! (every MCP client sees them), the SessionStart context (Claude Code), and `obmem guide`
//! (paste into AGENTS.md / CLAUDE.md for other agents).

pub const GUIDE: &str = "\
obmem is long-term memory for this project (repo vault) and this user (personal vault), stored as \
Obsidian notes that humans review.

Before a non-trivial task: memory_context with the task description. For precise lookups: memory_search when the task touches something a past session may have decided, learned \
or been told (Obsidian search syntax: words are ANDed, use OR for alternatives, \"phrases\", tag:, path:). \
Prefer what memory says over re-deriving it, but treat `draft` notes as unreviewed.

Record with memory_write, one idea per note, linking related notes with [[note-id]]:
- decision: what was chosen and why, alternatives considered, consequences
- fact / architecture: non-obvious behaviour of the system, with code_refs (path#symbol)
- procedure: steps that took effort to figure out
- feedback / preference: how the user wants agents to work (goes to the personal vault)
Do not record what the code or git history already says, one-off task details, or secrets.

Before finishing: memory_log 1-3 lines (what was done, what is left, what the next session must know).
After reading the code a note describes and finding it still true: memory_verify.
Human-authored notes are read-only: write a finding instead. Findings: memory_resolve (fixed/accepted).
If a tool says \"no obmem project here\", repeat the call with `project` set to the absolute path of your \
workspace root; the server remembers it for the rest of the session.";
