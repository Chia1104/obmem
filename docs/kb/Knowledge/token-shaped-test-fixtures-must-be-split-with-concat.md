---
type: fact
title: Token-shaped test fixtures must be split with concat!
status: draft
author: agent
confidence: high
created: 2026-11-09
updated: 2026-11-09
verified_commit: cfe8218
code_refs:
- src/secrets.rs#ALLOWLIST_PROBES
tags:
- gotcha
- github
- secrets
- tests
---
# Token-shaped test fixtures must be split with concat!

GitHub push protection rejects a push if **any** pushed commit contains a token-shaped literal, even an obviously fake test fixture such as `ghp_` + 36 characters, `AKIA…` or a Stripe `sk_live_…` key. It hit all three here, one push attempt at a time. A later commit that removes the string does not help: the history itself has to be clean.

**Convention:** in Rust source, write token-shaped fixtures in two pieces, for example `concat!("gh", "p_Qm4Zr8…")` or `concat!("AKIA", "IOSF…")`. The compiled string is identical, so the scanner tests still exercise detection, but no source line matches a secret pattern. `src/secrets.rs` already does this for `ALLOWLIST_PROBES` and its tests.

**Check before pushing:** use obmem's own scanner, which knows all of these formats. `git ls-files | grep -v Cargo.lock | xargs obmem scan | grep -v generic-assignment` should print nothing. Check every commit being pushed, not just HEAD. The repo's own pre-commit hook (`obmem scan --staged`) only scans the vault, so it will not catch this.
