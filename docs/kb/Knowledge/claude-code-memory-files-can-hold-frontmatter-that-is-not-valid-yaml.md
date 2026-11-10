---
type: fact
title: Claude Code memory files can hold frontmatter that is not valid YAML
status: active
author: agent
confidence: high
created: 2026-11-10
updated: 2026-11-10
verified_commit: f5b4b68
code_refs:
- crates/obmem-core/src/import.rs#quote_values
- crates/obmem-core/src/import.rs#plan
tags:
- import
- claude-code
approved_at: 2026-11-10
approved_by: auto-approve
---
Claude Code's file-based memory (`~/.claude/projects/<project>/memory/*.md`) is written by the agent, not by a YAML serializer. Most files quote `description:`, but some do not. Seen 2026-11-10 in the solyx project's memory folder (4 of 71 files):

- An unquoted `description:` with `: ` inside (for example `… oRPC v2 (2026-10-15): the user's decisions`) is a YAML error: `mapping values are not allowed in this context at line 2 column N`. The line number counts from the line after the opening `---`, so line 2 is line 3 of the file.
- An unquoted `description:` with ` #` inside (for example `PR #101`) parses, but YAML reads the rest as a comment. The value is cut at the `#` without any error.

What `obmem import-claude` does about it (since 2026-11-10): `import::quote_values` quotes such a top-level value before the file is parsed, so both cases import with the full description. It reads a trailing ` # comment` on such a line as part of the value, because in these files a `#` is far more often a PR number than a comment. A file that still does not parse stops the import, and the error names the file. `Note::parse` stays strict for obmem's own notes.
