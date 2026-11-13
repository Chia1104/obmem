---
type: procedure
title: 'Releasing a version: bump last, then tag the merged commit'
status: active
author: agent
confidence: high
created: 2026-11-10
updated: 2026-11-14
verified_commit: 8f6a67d
code_refs:
- .github/workflows/release.yml
- apps/obmem-gui/nsh.json
- .agents/skills/release/SKILL.md
tags:
- release
- procedure
approved_at: 2026-11-14
approved_by: auto-approve
---
The `release` skill (`.agents/skills/release/SKILL.md`) says what a release is and what the workflow does. These are the steps and the two mistakes made on 2026-11-10 with v0.1.0 and v0.1.1.

## Steps
1. Merge everything the release should contain.
2. Open the version bump as the last pull request, on top of all of it. It changes the version in three places: `[workspace.package] version` in the root `Cargo.toml`, the workspace crates in `Cargo.lock`, and `apps/obmem-gui/nsh.json`.
3. Optional: run the Release workflow by hand on `main`. Without a tag it builds every target and publishes nothing.
4. Tag the merged bump commit `v<version>` and push the tag.
5. Wait for the `verify` job. It installs the release with the install scripts on Linux, macOS and Windows and runs the binary.

## Why the bump goes last
The release build runs with `--locked`. A pull request that adds a crate writes it into `Cargo.lock` at the current version. If the bump merges before that pull request, `Cargo.lock` on `main` holds the new crate at the old version and the locked build fails. PR #17 (bump to 0.1.1) was stacked on PR #16 (new crate `obmem-update`) for this reason.

## A fix to release.yml needs the tag moved
A run started by a tag uses the workflow file of the tagged commit. Re-running a failed release does not pick up a fix merged afterwards. Either move the tag to the commit that has the fix, as was done twice for v0.1.0, or release a new version.

A re-run is safe for npm: the publish step skips versions that are already there. See [[a-new-npm-package-name-answers-404-for-minutes-after-its-first-publish]] before deciding that a publish failed.
