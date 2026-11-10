---
type: decision
title: obmem update replaces only standalone installs, by running the release's install script
status: active
author: agent
confidence: high
created: 2026-11-10
updated: 2026-11-10
verified_commit: 7c251b0
code_refs:
- crates/obmem-update/src/lib.rs#Install
- crates/obmem-update/src/lib.rs#latest
- crates/obmem-update/src/lib.rs#install
- scripts/install.sh
- scripts/install.ps1
tags:
- release
- update
- architecture
approved_at: 2026-11-10
approved_by: auto-approve
---
# obmem update replaces only standalone installs, by running the release's install script

## Context
obmem reaches a machine in several ways: npm (`@chia1104/obmem`), the install scripts of a GitHub release (`scripts/install.sh`, `scripts/install.ps1`), or a build from a clone. They all carry the same binary, but whoever installed it owns the file. The user wanted an `obmem update` command next to these install channels (2026-11-10).

## Decision
- `obmem update` first decides how the running binary was installed, from its own path (`Install::of`): inside `node_modules` is npm, cargo's bin folder or a `target` folder is a source build, anything else is standalone.
- Only a standalone binary is replaced. For npm and source builds the command prints what to run.
- The replacement is done by the install script of the new release, downloaded and run with `OBMEM_VERSION` and `OBMEM_INSTALL_DIR` set to the binary's folder. The script picks the archive, checks `SHA256SUMS` and swaps the file.
- The newest version comes from npm's `latest` tag for an npm install, and from GitHub's latest release for the others.
- Binaries, checksums and install scripts are files of the GitHub release. No bucket of our own stores them.

## Alternatives considered
- Replace the binary in place for every install: npm would keep recording the old version and put the old file back on its next install.
- Download and unpack the archive in Rust (reqwest, flate2, tar, zip, a self-replace crate): a second implementation of what the install scripts do, in another language, with more dependencies.
- Embed the install script in the binary instead of downloading the release's: an old binary would then install new releases with old logic. With the downloaded script, the contract with past versions is only that every release has both scripts and that they honour the two variables.
- An install receipt written by the installer: not needed, because the path already tells.
- cargo-dist: it would replace the release pipeline that had just been made to work, and builds npm packages differently.
- Checking for updates in hooks or at start-up: hooks run in every agent turn and must not touch the network.

## Consequences
- A new install channel adds a variant to `Install` and one row of path detection.
- Every release must keep `install.sh` and `install.ps1` and the two variables, or old binaries can no longer update.
- With several installs on one machine the first on `PATH` runs. `obmem status` prints the binary and how it was installed, and says when `obmem` on `PATH` is another one.
- Checked on 2026-11-10 on macOS: a binary built as 0.0.9 updated itself to the real v0.1.0 release as a standalone install, and only printed the command when placed in a `node_modules` or `target` folder. The Windows path (`powershell -File`) has never run.
