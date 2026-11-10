---
type: fact
title: A new npm package name answers 404 for minutes after its first publish
status: active
author: agent
confidence: medium
created: 2026-11-10
updated: 2026-11-10
verified_commit: 38a6b4e
code_refs:
- .github/workflows/release.yml
tags:
- release
- npm
- gotcha
approved_at: 2026-11-10
approved_by: auto-approve
---
# A new npm package name answers 404 for minutes after its first publish

# A new npm package name answers 404 for minutes after its first publish

Seen on 2026-11-10 with the first publish of the six `@chia1104/obmem*` packages (v0.1.0):
- `npm publish` reported success for all six in the release log.
- `registry.npmjs.org/@chia1104/obmem` still answered 404 about four minutes later.
- Five packages became installable within about 15 minutes. `@chia1104/obmem-linux-x64` took longer and showed only a placeholder version, `0.0.0-stage`, in the meantime. Until it went live, an install on Linux x64 got the launcher with no binary.
- The placeholder stays in the version list afterwards, next to the real versions.

The session read this as npm staging the first publish of a new name. That reading is a guess; the delay and the placeholder are what was observed.

**How to apply:** after the first release of a new package name, poll the registry before calling the publish failed, and check every platform package, not only the launcher. The publish step in `release.yml` skips versions already on npm, so re-running a release is safe. Whether a later version of an existing name is delayed too was not measured: 0.1.1 was only looked at hours later.

Related: [[npm-packages-are-published-under-the-chia1104-scope]].
