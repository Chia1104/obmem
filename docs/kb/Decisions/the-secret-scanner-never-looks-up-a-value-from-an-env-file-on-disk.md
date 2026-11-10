---
type: decision
title: The secret scanner never looks up a value from an env file on disk
status: active
author: agent
confidence: high
created: 2026-11-10
updated: 2026-11-10
verified_commit: 0ce6fee
code_refs:
- crates/obmem-core/src/secrets.rs#is_relative_path
- crates/obmem-core/src/secrets.rs#is_existing_path
- crates/obmem-core/src/secrets.rs#collect_known_values
tags:
- security
- secrets
- windows
approved_at: 2026-11-10
approved_by: auto-approve
---
# The secret scanner never looks up a value from an env file on disk

# The secret scanner never looks up a value from an env file on disk

## Context
The scanner treats every value in the project's `.env*` files as a known secret, except paths, because paths are everywhere in notes. It used to decide "is this a path" with `Path::exists()` on any value that starts with `/`, `~` or `.`. A cloned repo writes those files. On Windows, looking up `//host/share/x` opens an SMB session to that host and sends the user's NTLM credentials (security finding F4 of 2026-11-09, CHI-69). It happened whenever a store opened.

## Decision
- A value from an env file is only compared as text.
- A value that starts with `./`, `../` or `~/` (or the same with `\`) is a path by its shape and is left out. No token format starts that way.
- Any other value from an env file counts as a known value, absolute paths included.
- The lookup stays for values from the process environment, and only for values that start with `/`. The user controls that environment.

## Alternatives considered
- Reject `//`, `\\` and `/net/` before the lookup: not enough. A relative value can climb to `/net` with `../`, and a cloned repo can carry a symlink that points there.
- Decide every path by shape, absolute ones too: a base64 secret can start with `/` and contain more of them, so some real secrets would be dropped from the known values. An AWS secret access key has no format rule to fall back on.
- Look up only paths inside the repo, one component at a time and refusing symlinks: much more code for the one case that the shape rule already covers.
- Drop the lookup for the process environment too: one rule fewer, but every `*_KEY_FILE=/abs/path` variable would turn its path into a secret value.

## Consequences
- A project whose `.env` holds an absolute path, such as `SSL_CERT_FILE=/etc/ssl/cert.pem`, gets a `known-secret-value` hit when a note quotes that path. The fix for the user is the fingerprint in the allowlist.
- `~/…` values are left out now. Before, they were looked up without expanding `~`, so they almost never counted as paths.
- The rule is in the security invariants of `crates/obmem-core/AGENTS.md`, with a test.
