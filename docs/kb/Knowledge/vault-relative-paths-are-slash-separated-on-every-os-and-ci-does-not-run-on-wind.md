---
type: fact
title: Vault-relative paths are slash-separated on every OS, and CI does not run on Windows
status: active
author: agent
confidence: medium
created: 2026-11-17
updated: 2026-11-17
verified_commit: f852562
code_refs:
- crates/obmem-core/src/store.rs#relative
- crates/obmem-core/src/headless.rs#graph
- crates/obmem-core/src/init.rs#toml_string
tags:
- windows
- paths
- gotcha
approved_at: 2026-11-17
approved_by: human (cli)
---
Every vault-relative path that obmem compares or stores is `/`-separated: the catalog's `Meta::rel`, the headless backend's link graph, session logs, sync manifests and `NoteRef::At`. On Windows, `Path::strip_prefix(..).to_string_lossy()` gives `Decisions\x.md`, which never equals `Decisions/x.md`. Add `.replace('\\', "/")` wherever a path becomes a vault-relative string.

Found on 2026-11-17: `Store::relative` kept the backslashes, so on Windows with the headless backend `memory_get` printed `links: none | backlinks: none` for notes that had links.

Other Windows breakages found the same day, with the same cause of building text from paths by hand:
- `init` wrote `vault = "C:\Users\..."` into the user's config. In a TOML basic string `\U` is an escape, so every obmem command failed to parse the file. Write TOML values through `toml_edit` (`init::toml_string`), and JSON through `serde_json`.
- A checkout with `core.autocrlf=true` turned `SKILL.md` into CRLF, which `include_str!` would embed in a Windows build. `.gitattributes` (`* text=auto eol=lf`) keeps every file LF.

**Update (2026-11-17, branch `ci/windows`, commit f852562):** until then CI (`.github/workflows/ci.yml`) ran only on ubuntu-latest and macos-latest. That branch adds a `windows` job that runs the workspace's clippy and tests. Once it is merged, "CI does not run on Windows" in this note's title no longer holds.
