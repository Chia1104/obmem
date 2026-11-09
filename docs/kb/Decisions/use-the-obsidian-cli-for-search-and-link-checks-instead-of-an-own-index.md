---
type: decision
title: Use the Obsidian CLI for search and link checks instead of an own index
status: active
author: agent
confidence: high
created: 2026-11-09
updated: 2026-11-09
verified_commit: e9961bd
code_refs:
- crates/obmem-core/src/obsidian.rs#Api
- crates/obmem-core/src/catalog.rs#Catalog
- crates/obmem-core/src/store.rs#search
tags:
- obsidian
- architecture
approved_at: 2026-11-09
approved_by: human (gui)
verified_at: 2026-11-09
verified_by: agent
---
## Context
obmem first shipped its own SQLite FTS5 (trigram) index for search, backlinks and link resolution. Obsidian already does all of this, and since 1.12 it has an official CLI.

## Decision
The user wants no reimplementation of what Obsidian already does. obmem calls the Obsidian CLI for:
- `memory_search` (`search:context`)
- `memory_get` backlinks (`backlinks`)
- audit `broken-link` (`unresolved`)
- audit `orphan` (`orphans`)

Only what Obsidian has no command for is done locally by reading files:
- the id / title / status catalog
- secret scanning
- schema, stale, supersede, duplicate and code-drift rules
- ranking for `memory_context`, because Obsidian search does not rank

## Alternatives considered
- Keep the SQLite index: it works headless and ranks with BM25, but it duplicates Obsidian.
- Make Obsidian an optional backend: two code paths for the same feature.

## Consequences
- The Obsidian app must be installed with the CLI enabled. When the app is closed, obmem starts it in the background and retries, because the CLI does not start it, despite the docs.
- Every vault must be registered once by hand ("Open folder as vault").
- Search results are not relevance-ranked.
- The nightly headless cron audit was dropped.
- When Obsidian is unreachable, obmem degrades instead of failing: it skips that vault in search, shows backlinks as "unavailable" and skips the link rules in the audit.

See [[obsidian-cli-behaviour]].
