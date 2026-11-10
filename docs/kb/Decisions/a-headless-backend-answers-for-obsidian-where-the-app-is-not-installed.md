---
type: decision
title: A headless backend answers for Obsidian where the app is not installed
status: active
author: agent
confidence: high
created: 2026-11-10
updated: 2026-11-10
verified_commit: 38a6b4e
code_refs:
- crates/obmem-core/src/obsidian.rs#Backend
- crates/obmem-core/src/headless.rs#Headless
- crates/obmem-core/src/query.rs#parse
- crates/obmem-core/src/links.rs#Names
- crates/obmem-core/src/store.rs#open_with
tags:
- obsidian
- architecture
- remote
approved_at: 2026-11-10
approved_by: auto-approve
verified_at: 2026-11-10
verified_by: agent
---
# A headless backend answers for Obsidian where the app is not installed

## Context
Remote agents (cloud sandboxes, servers) run obmem on a machine that has no Obsidian app. `memory_search`, `memory_context` (it searches through `Store::search`), `memory_get` backlinks and the audit's `broken-link` and `orphan` rules all went through the Obsidian CLI, so none of them worked there.

## Decision
The user chose (2026-11-10) to make the Obsidian CLI one of two backends behind `obsidian::Api`:
- `Cli` asks the app. `Headless` reads the vault files itself.
- `Backend::detect` picks one when a store opens: `Cli` when the Obsidian CLI command (`OBMEM_OBSIDIAN_BIN`, else `obsidian`) is on `PATH`, else `Headless`.
- The choice holds for the life of the store. A failed CLI call never falls back to `Headless`.

This replaces one part of [[use-the-obsidian-cli-for-search-and-link-checks-instead-of-an-own-index]]: that note rejected "Obsidian as an optional backend". The rest of it still holds. On a machine with Obsidian, obmem uses Obsidian and reimplements nothing, and there is still no index.

## Alternatives considered
- Leave search failing on remote machines and tell agents to use `memory_context`: `memory_context` needs search too, so nothing would work.
- Fall back to `Headless` whenever a CLI call fails: one process would mix two sets of results, and a desktop user would not learn that Obsidian is broken.
- A config switch for the backend: nothing needs it yet. `OBMEM_OBSIDIAN_BIN` set to a missing path already forces `Headless`.

## Consequences
- Two code paths answer the same questions. A new `Api` method needs both.
- `Headless` supports part of Obsidian's search syntax (listed in `query.rs`) and refuses the rest with an error. `Store::search` and `context` must only send queries inside that part.
- `Headless` counts only `[[wikilinks]]` as links, not Markdown links.
- A desktop whose `PATH` lacks the `obsidian` command now gets `Headless` instead of a "CLI not found" error. `obmem status` shows which backend is in use (`search:`).
- Checked on 2026-11-10 against Obsidian 1.14 on this repo's two vaults: 15 search queries returned the same notes from both backends, and so did the `orphan` rule (8 notes) and the backlinks of 5 notes. Neither backend found a broken link, so that rule was compared only on an empty result.
