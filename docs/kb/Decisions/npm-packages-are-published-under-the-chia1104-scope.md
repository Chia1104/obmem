---
type: decision
title: npm packages are published under the @chia1104 scope
status: active
author: agent
confidence: high
created: 2026-11-10
updated: 2026-11-10
verified_commit: 38a6b4e
code_refs:
- npm/obmem/package.json
- npm/build.mjs
tags:
- release
- npm
approved_at: 2026-11-10
approved_by: auto-approve
---
# npm packages are published under the @chia1104 scope

# npm packages are published under the @chia1104 scope

## Context
The first release (v0.1.0, 2026-11-10) published unscoped packages: one per platform, named `obmem-<platform>-<arch>`, plus the launcher `obmem`. The five platform packages went through. npm then refused the name `obmem` as too similar to the existing package `mem`.

## Decision
The user chose to move every package to the `@chia1104` scope: `@chia1104/obmem` and `@chia1104/obmem-<platform>-<arch>`. The command people run is still `obmem`. The name in `npm/obmem/package.json` names all six, so a rename is one edit.

## Alternatives considered
None were weighed. The user picked the scope as soon as npm refused the name.

## Consequences
- Install and docs use `npm install -g @chia1104/obmem`.
- The five unscoped platform packages stay on npm at 0.1.0 with no launcher. They never get a newer version. Deprecating them is tracked in Linear.
- `Install::of` in obmem-update only looks for `node_modules` in the binary's path, so the scope does not matter to `obmem update`.

Related: [[obmem-update-replaces-only-standalone-installs-by-running-the-release-s-install-]].
