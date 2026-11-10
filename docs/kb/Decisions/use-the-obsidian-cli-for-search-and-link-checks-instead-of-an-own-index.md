---
type: decision
title: Use the Obsidian CLI for search and link checks instead of an own index
status: active
author: agent
confidence: high
created: 2026-11-09
updated: 2026-11-10
verified_commit: 38a6b4e
code_refs:
- crates/obmem-core/src/obsidian.rs#Api
- crates/obmem-core/src/catalog.rs#Catalog
- crates/obmem-core/src/store.rs#search
tags:
- obsidian
- architecture
approved_at: 2026-11-10
approved_by: auto-approve
verified_at: 2026-11-09
verified_by: agent
---
## Context
obmem first shipped its own SQLite FTS5 (trigram) index for search, backlinks and link resolution. Obsidian already does all of this, and since 1.12 it has an official CLI.

## Decision
The user wants no reimplementation of what Obsidian already does. On a machine that has Obsidian, obmem calls the Obsidian CLI for:
- `memory_search` (`search:context`)
- `memory_get` backlinks (`backlinks`)
- audit `broken-link` (`unresolved`)
- audit `orphan` (`orphans`)

Only what Obsidian has no command for is done locally by reading files:
- the id / title / status catalog
- secret scanning
- schema, stale, supersede, duplicate and code-drift rules
- ranking for `memory_context`, because Obsidian search does not rank

**Changed on 2026-11-10.** A machine without the `obsidian` command answers the same four questions from the vault files. See [[a-headless-backend-answers-for-obsidian-where-the-app-is-not-installed]]. There is still no index.

## Alternatives considered
- Keep the SQLite index: it works headless and ranks with BM25, but it duplicates Obsidian.
- Make Obsidian an optional backend: rejected on 2026-11-09 as two code paths for the same feature. The user accepted that cost one day later, for remote machines only (see the note linked above).

## Consequences
These hold on a machine that has the `obsidian` command:
- The Obsidian app must be installed with the CLI enabled. When the app is closed, obmem starts it in the background and retries, because the CLI does not start it, despite the docs.
- Every vault must be registered once by hand ("Open folder as vault").
- When Obsidian is unreachable, obmem degrades instead of failing: it skips that vault in search, shows backlinks as "unavailable" and skips the link rules in the audit. It never switches to the built-in search.

These hold everywhere:
- Search results are not relevance-ranked.
- The nightly headless cron audit was dropped.

See [[obsidian-cli-behaviour]].
