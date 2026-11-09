---
type: fact
title: rustfmt --check exits 0 on stdin even when it prints a diff
status: active
author: agent
confidence: high
created: 2026-11-10
updated: 2026-11-10
verified_commit: e5a6be2
code_refs:
- .githooks/pre-commit
tags:
- tooling
- rustfmt
- git-hooks
approved_at: 2026-11-10
approved_by: auto-approve
---
# rustfmt --check exits 0 on stdin even when it prints a diff

With stdin input, `rustfmt --check` prints the diff but still exits 0 (seen with rustfmt 1.10.0-stable). A check that relies on its exit code silently passes everything.

The pre-commit hook checks the staged content of each `.rs` file, so it has to read from stdin: passing the file path would check the working tree, and rustfmt would follow `mod` declarations into other (possibly unstaged) files. Instead the hook formats the staged blob (`git show :file | rustfmt --edition 2024`), hashes the output with `git hash-object --stdin` and compares it with `git rev-parse :file`. A file that does not parse yields empty output, so it fails too.

`--edition 2024` must match `edition` in the root Cargo.toml; `cargo fmt` reads it from there, a bare `rustfmt` does not.
