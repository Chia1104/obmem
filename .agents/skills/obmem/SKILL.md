---
name: obmem
description: Project and personal memory through obmem's memory_* tools. Load to search what past sessions decided or learned, to record a decision, a non-obvious fact, a procedure or the user's feedback as a note, or to act on a finding.
metadata:
  managed-by: obmem setup rewrites this file
---

# obmem

Memory is two vaults of Obsidian notes that people review. The **project vault** is the team's: in the repo and shared through git, or a private folder outside the repo. The **personal vault** is this user's, for every project.

The routine is two calls: `memory_context` before a task, `memory_log` before finishing. This skill is everything between.

## Recall

- `memory_context` ranks notes against a task description. Start there.
- `memory_search` is exact. Use it when the task touches something a past session may have decided, learned or been told. It takes Obsidian search syntax: words are ANDed, `OR` for alternatives, `"exact phrase"`, `tag:`, `path:`, `-exclude`. Results are unranked, so search for specific words.
- `memory_get` reads one note in full, with the notes that link to it.
- Prefer what a note says over re-deriving it. A `draft` note is unreviewed: check it before relying on it.

## Record

Record what a future session could not learn from the code or the git history. One idea per note, linked to related notes with `[[note-id]]`.

| Type | Holds | Vault |
| --- | --- | --- |
| `decision` | what was chosen and why, the alternatives considered, the consequences | project |
| `fact`, `architecture` | non-obvious behaviour of the system | project |
| `procedure` | steps that took effort to work out | project |
| `feedback`, `preference` | how the user wants agents to work | personal |

- Search before you write. When a note on the idea exists, replace it with `update_id`.
- Give a note about code its `code_refs`, as `path#symbol`. The audit uses them to notice when the code moved on. Name the symbol: a whole-file ref is flagged by every commit to that file.
- Date an observation that can expire: "Checked on 2026-11-10 with Obsidian 1.14".
- A note a person wrote is read-only. To disagree with it, write a `finding` that names it.
- Keep secrets and one-off task details out. A write that holds a secret is refused.

## Log

`memory_log` is the session log: one to three lines on what was done, what is left, and what the next session must know. The log is for handing over. Knowledge that should last goes in a note.

## Keep notes true

- You read the code a note describes and the note is still true: `memory_verify`.
- You fixed what a finding reports, or it is real and stays open: `memory_resolve` with `fixed` or `accepted`. Rejecting a finding is for a person.
- A note disagrees with the code, or `memory_audit` reports problems: the `obmem-refresh` skill.
