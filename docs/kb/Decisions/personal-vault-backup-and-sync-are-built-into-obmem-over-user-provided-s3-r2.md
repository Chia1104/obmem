---
type: decision
title: Personal vault backup and sync are built into obmem over user-provided S3/R2
status: active
author: agent
confidence: high
created: 2026-11-09
updated: 2026-11-09
verified_commit: 03a9865
code_refs:
- crates/obmem-sync/src/lib.rs#backup
- crates/obmem-sync/src/files.rs#scan
- crates/obmem-core/src/sessions.rs#recorded_root
tags:
- sync
- backup
- encryption
approved_at: 2026-11-09
approved_by: auto-approve
---
## Context
The user wants the personal (global) vault backed up automatically and kept in sync between macOS and Windows. Obsidian Sync covers this but is paid, so obmem builds it itself. The project vault is not included, because git already syncs it.

## Decision
- A new crate, `obmem-sync`. It depends on obmem-core, which has no network or tokio, and the CLI and GUI share it.
- The remote is a bucket the user sets up (S3, R2 or MinIO). It holds content-addressed, immutable blobs (id = HMAC of the content) and numbered snapshot manifests (`snapshots/<seq>.age`). **Every write is create-only.** A new snapshot takes the next number; when another device took it first, the backup retries on top of that snapshot. There is no `HEAD` object: the planned conditional HEAD update was dropped as unnecessary, because listing is strongly consistent on S3 and R2. Each snapshot is a restore point.
- Encryption uses the `age` crate. A random vault key (an X25519 identity, with the MAC key derived from it) is wrapped in `key.age` with a **passphrase the user chooses**. There is no separate recovery key. Changing the passphrase only rewraps `key.age`. Each device keeps the unlocked key and the S3 credentials in one OS keychain entry (`keyring`).
- Paths use `/` and NFC. Names Windows cannot hold are skipped at backup and refused at restore, and so are names that collide by case. Bytes are stored unchanged, with no CRLF conversion.
- `.obsidian/` is backed up with an allowlist modeled on Obsidian Sync's defaults: top-level settings JSON, `themes/` and `snippets/`. `workspace*.json` is never backed up, and `plugins/` with `community-plugins.json` only when `[sync] community_plugins = true`. Other dot-folders are never backed up.
- Sync in P2 is planned as a three-way merge (last-synced snapshot / local files / latest snapshot). When both sides changed a file, both versions are kept and a finding is opened.

## Alternatives considered
- Obsidian Sync: paid.
- Remotely Save (a community plugin): syncs only while Obsidian is running, and knows nothing about findings or session logs.
- A recovery key in addition to the passphrase: the user declined. If the passphrase is forgotten, the backup cannot be read, and the only fallback is the plaintext copies on the devices.

## Consequences
- Session logs record the textual project root (sessions.rs `recorded_root`). The same project on another device has a different root, so its logs are hidden, and logs for the same date collide. Plan: put the device in the log file name. Showing another device's logs needs an explicit link the user makes, never an automatic one (see F5/F6).
- Phases: P1 `obmem sync setup|backup|list|restore` (done on branch feat/vault-backup), P2 two-way `obmem sync`, P3 hooks (pull at session start, background push at Stop) and the GUI, P4 retention and prune (deleting old snapshots and unreferenced blobs is the only operation that removes objects).
