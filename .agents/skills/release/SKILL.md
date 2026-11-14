---
name: release
description: How an obmem release is built and published (version tag, release workflow, npm packages, install scripts). Load before bumping the version, tagging a release, re-running or changing `.github/workflows/release.yml`, adding a build target, or changing `npm/` or `scripts/install.*`.
---

# Releasing obmem

A release is a `v<version>` tag whose version equals `[workspace.package] version` in the root `Cargo.toml`.

## What the release workflow does

- `.github/workflows/release.yml` builds the binary for every target with the `dist` profile.
- It publishes the npm packages that `npm/build.mjs` assembles: one per platform, plus `@chia1104/obmem`, whose `bin/obmem.js` runs the one npm installed. The name in `npm/obmem/package.json` names them all.
- It attaches the binaries, `SHA256SUMS` and the install scripts (`scripts/install.sh`, `scripts/install.ps1`) to a GitHub release.
- Its `verify` job then installs that release with the scripts on Linux, macOS and Windows and runs the binary, `obmem update --check` included.
- It publishes to npm with trusted publishing: no token is stored. Every `@chia1104/obmem*` package on npmjs.com names `Chia1104/obmem`, the workflow `release.yml` and the environment `release` as its trusted publisher, so renaming the workflow file or the environment stops publishing.

## Rules

- Run the workflow by hand to check that every target builds: without a tag it publishes nothing.
- `npm/obmem/targets.json` is the one list of platforms. A new target also needs a row in the workflow's matrix.
- A new target is a new npm package, and npm only lets a package that already exists have a trusted publisher. Its first publish needs a token or an `npm publish` by hand. Add its trusted publisher after that.

## Steps and past mistakes

The order of merge, version bump and tag, and what to do when a tagged run fails, are in the project memory: run memory_search `releasing a version`.
