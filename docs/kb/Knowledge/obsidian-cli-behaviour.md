---
type: fact
title: Obsidian CLI behaviour
status: active
author: agent
confidence: high
created: 2026-11-09
updated: 2026-11-14
verified_commit: 3fa0335
code_refs:
- crates/obmem-core/src/obsidian.rs#vault_id
- crates/obmem-core/src/obsidian.rs#is_empty_result
- crates/obmem-core/src/obsidian.rs#app_missing
- crates/obmem-core/src/obsidian.rs#parse_links
- crates/obmem-core/src/store.rs#search
tags:
- obsidian
- gotcha
approved_at: 2026-11-14
approved_by: auto-approve
verified_at: 2026-11-10
verified_by: agent
---
Non-obvious behaviour of the official Obsidian CLI (verified on Obsidian 1.14.5) that `crates/obmem-core/src/obsidian.rs` depends on:

- **App must be running:** if Obsidian is closed, the CLI does not start it, although the docs say it does. It prints "The CLI is unable to find Obsidian…" on stderr and exits 1. obmem starts the app in the background (`open -g -a Obsidian` on macOS) and retries.
  - For about 1-2 s after launch, commands answer `Error: Command "…" not found` until they register, so obmem keeps retrying through that too.
  - All registered vaults become queryable, not only the one in front.
- **Vault selection:** run from a folder that is not a registered vault, the CLI silently uses the *active* vault. So obmem always passes `vault=<id>`, looked up by canonical path in Obsidian's `obsidian.json`. The id also avoids name clashes, since every repo's project vault is called `kb`.
- **Registration:** vaults cannot be registered from the CLI or with `obsidian://open?path=`. A person must use "Open folder as vault". "Create new vault" nests the folder one level deeper.
- **Errors and empty results:** errors arrive as plain text on stdout with exit code 0 (`Error: …`, `Vault not found.`). Empty results are also plain text (`No matches found.`, `No backlinks found.`, `No links found.`), even with `format=json`. The "app not running" case is the exception: stderr and exit 1.
- **Output shapes:**
  - `search:context format=json`: `[{file, matches: [{line, text}]}]`. Lines are 1-based and a line repeats once per occurrence.
  - `unresolved verbose format=json`: `sources` is one `, `-separated string.
  - `orphans`: one path per line.
  - `links path=<note>` (checked on 2026-11-14): one target per line, sorted by name, each target once. It has no JSON form: `format=json` prints the same lines. A file is its vault-relative path, and other files than notes are listed too (`未命名.canvas`). A link that opens nothing is `<link text> (unresolved)`. The note's link to itself is listed, so obmem drops it. `total` prints only the count.
- **Search operators:** `[type:x]`, `-[status:x]`, `-path:X`, `tag:x` and `line:( )` all work, and words are ANDed. `a OR b -x` parses as `a OR (b -x)`, so obmem wraps the user's query in parentheses before adding its filters.
- **What counts as a link:** links inside code are ignored, links in frontmatter count, and so do Markdown links (`[text](Index.md)`). A `[[link]]` in a finding would count as a backlink and rescue an orphan, so findings name their note by path.
- **No relevance ranking** in search results.
- **Speed:** a call takes about 5 ms while the app is running (2026-11-14), so a few calls per `memory_context` cost nothing a person notices.
- **A new file may not be known yet:** seen once on 2026-11-14. Three seconds after a note was written, `links` answered `No links found.` and the next call, `links total`, counted its links. It did not happen again in three more tries, so the cause is not known.

Background: [[use-the-obsidian-cli-for-search-and-link-checks-instead-of-an-own-index]].
