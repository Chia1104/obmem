---
type: fact
title: Vault-relative paths are slash-separated on every OS, and CI does not run on Windows
status: active
author: agent
confidence: medium
created: 2026-11-17
updated: 2026-11-17
verified_commit: 8ff6e44
code_refs:
- crates/obmem-core/src/store.rs#relative
- crates/obmem-core/src/headless.rs#graph
- crates/obmem-core/src/catalog.rs#Catalog
tags:
- windows
- paths
- gotcha
approved_at: 2026-11-17
approved_by: human (cli)
---
# Vault-relative paths are slash-separated on every OS, and CI does not run on Windows

Every vault-relative path that obmem compares or stores is `/`-separated: the catalog's `Meta::rel`, the headless backend's link graph, session logs, sync manifests and `NoteRef::At`. On Windows, `Path::strip_prefix(..).to_string_lossy()` gives `Decisions\x.md`, which never equals `Decisions/x.md`. Add `.replace('\\', "/")` wherever a path becomes a vault-relative string.

Found on 2026-11-17: `Store::relative` kept the backslashes. On Windows with the headless backend, `memory_get` therefore printed `links: none | backlinks: none` for notes that had links, and `NoteRef::At` could not find a note by a path built that way. Two tests in `store::tests` (`works_without_obsidian` and `a_person_approves_drafts_only`) had been failing on Windows for a while without anyone noticing.

That went unnoticed because CI (`.github/workflows/ci.yml`) runs only on ubuntu-latest and macos-latest. A change that touches paths has to be tested on a Windows machine by hand (see CHI-67).
