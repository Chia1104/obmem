//! What every agent session is told about the memory. This text is always in context, through
//! two channels: the MCP server instructions (every MCP client sees them) and the session-start
//! context (the agents with a hook). So it holds only what every task needs: what obmem is, the
//! call that opens a task and the call that closes it. Everything else is in the skills that
//! `obmem setup` installs (`crates/obmem-agents/skills/`), which an agent loads when it needs them.

pub const GUIDE: &str = "\
obmem is long-term memory for this project (project vault) and this user (personal vault), stored as \
Obsidian notes that humans review.

Before a non-trivial task: memory_context with the task description. Prefer what memory says over \
re-deriving it, but treat `draft` notes as unreviewed.
Before finishing: memory_log, 1-3 lines (what was done, what is left, what the next session must know).

Skills hold the rest. `obmem`: load it to search memory, or to record a decision, a non-obvious \
fact, a procedure or the user's feedback. `obmem-refresh`: load it when a note disagrees with the \
code, or memory_audit reports problems.";
