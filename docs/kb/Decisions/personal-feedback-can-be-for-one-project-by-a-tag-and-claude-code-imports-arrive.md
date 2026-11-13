---
type: decision
title: Personal feedback can be for one project by a tag, and Claude Code imports arrive that way as drafts
status: active
author: agent
confidence: high
created: 2026-11-14
updated: 2026-11-14
verified_commit: d565450
code_refs:
- crates/obmem-core/src/catalog.rs#global_guidance
- crates/obmem-core/src/config.rs#project_tag
- crates/obmem-core/src/import.rs#standing
- crates/obmem-core/src/store.rs#duplicate_of
- crates/obmem-core/src/store.rs#WriteRequest
- crates/obmem-mcp/src/lib.rs#WriteArgs
tags:
- import
- guidance
approved_at: 2026-11-14
approved_by: auto-approve
---
## Context

On 2026-11-14 the user imported Claude Code memory in a third project (chia1104.dev, after solyx and obmem) and found the personal vault's guidance in conflict. `Global/Feedback` held 17 active notes, 14 of them imported, and every session in every project listed all of them as the user's guidance: solyx's branch rule in obmem, chia1104.dev's enum convention in solyx. Two imports of the same feedback (`reply-in-chinese` from solyx, `reply-in-traditional-chinese` from chia1104.dev) were both active.

Three causes:
- The import sent every `feedback` memory to the global vault as guidance for all projects. Claude Code keeps one memory folder per project, so its feedback is what the user said while working in that project.
- The import compared file names only. It skipped obmem's title check with `force_new`, and a memory with a taken name was reported as "already exists" whatever it said.
- With `[review] auto_approve` on, imports were written `active`, so nobody read them before they became guidance.

Agents had the first problem too: feedback an agent recorded about one project through `memory_write` was guidance everywhere.

## Decision

- A feedback or preference note in the global vault that carries `project/<name>` tags is guidance in those projects only. A note without one is guidance everywhere. `<name>` is the slug of the project's name (`Config::project_tag`), the tag session logs already had. `Catalog::global_guidance` applies it.
- A writer asks for the tag with `WriteRequest::this_project_only` and never spells it: `Store::write` adds the project's tag to a new note for the global vault. `memory_write` has the argument, and the `obmem` skill tells agents to pass it when they cannot tell.
- The import asks for it for every `feedback` memory. A `user` memory gets none.
- Where an existing note holds is the user's to change, in Obsidian. An agent's edit keeps the note's project tags whatever tags it names, and is refused when it asks for a project the note does not hold in.
- An imported note is a draft whatever `auto_approve` says (`WriteRequest::needs_review`).
- `import::standing` says what keeps a memory out, as a `Skip`: imported before (the note there names its memory file), another note has its name, or a note of the same type and scope has a similar title (`Store::duplicate_of`, the check `write` makes). The CLI's `--dry-run` and the desktop app ask it before anything is written.

## Alternatives considered

- **Imported feedback to the project vault.** Refused: that vault is shared with the team and can be in a public repo, and feedback such as the reply language or which model subagents use is personal.
- **A folder per project in the personal vault** (`Projects/<name>/Feedback`, next to `Sessions`). Refused: it needs a third scope in `Store::vault_for` and every caller of it, and moving a note between "this project" and "everywhere" becomes a file move. With a tag the person adds or removes one line in Obsidian, and one note can hold in two projects.
- **Agents write the tag themselves in `tags`.** Refused: a tag that matches no project hides the note from every session start, and the slug is easy to get wrong (`chia1104.dev` is `project/chia1104-dev`). The store knows the project and writes the tag.
- **A looser title check for imports.** Not done. Measured on the 132 memories of the three projects: the two reply notes score 0.39 (trigram Jaccard), below the 0.5 that `write` refuses at, and the next pair scores 0.30. One pair is not enough to move a threshold that `memory_write` shares. The draft rule is what keeps such a pair from both becoming guidance.

## Consequences

- The tag is the only thing that scopes a note. One a person mistypes, or the tag of a project that was renamed, hides the note from every session start. Nothing reports that yet.
- An agent cannot make a note hold everywhere, or in one more project. The user removes or adds the tag.
- `memory_search` and `memory_context` do not look at the tag: another project's feedback can still be found by its words.
- The project name is the repo's (`.obmem.toml`), so a clone that takes another project's name is shown that project's guidance. Before the tag every project was shown it.
- The session start still lists at most 30 guidance notes, in no stated order.
