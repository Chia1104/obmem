---
type: decision
title: Personal vault backup and sync are built into obmem over user-provided S3/R2
status: active
author: agent
confidence: high
created: 2026-11-09
updated: 2026-11-10
verified_commit: 38a6b4e
code_refs:
- crates/obmem-sync/src/sync.rs#merge
- crates/obmem-sync/src/files.rs#scan
- crates/obmem-core/src/audit.rs#SYNC_CONFLICT_MARK
- crates/obmem-core/src/store.rs#Store::log
tags:
- sync
- backup
- encryption
approved_at: 2026-11-10
approved_by: auto-approve
---
## Context
The user wants the personal (global) vault backed up automatically and kept in sync between macOS and Windows. Obsidian Sync covers this but is paid, so obmem builds it itself. The project vault is not included, because git already syncs it.

## Decision
- A new crate, `obmem-sync`. It depends on obmem-core, which has no network or tokio, and the CLI and GUI share it.
- The remote is a bucket the user sets up (S3, R2 or MinIO). It holds content-addressed blobs (id = HMAC of the content) and numbered snapshot manifests (`snapshots/<seq>.age`). Snapshots and `key.age` are create-only. Blobs were create-only at first and are a plain put since retention was added: see [[sync-retention-keeps-a-week-then-dailies-then-weeklies-blobs-are-collected-after]]. There is no `HEAD` object: the latest snapshot is the highest number in a listing, which is strongly consistent on S3 and R2. Every sync that changes something is a snapshot, so backup and sync are one feature.
- Encryption uses the `age` crate. A random vault key (an X25519 identity, with the MAC key derived from it) is wrapped in `key.age` with a **passphrase the user chooses**. There is no recovery key. Each device keeps the unlocked key and the S3 credentials in one OS keychain entry. A machine without a keychain takes them from the environment: see [[remote-machines-sync-with-credentials-from-the-environment-and-an-ephemeral-sett]].
- Sync is a three-way merge: the device's base (the last snapshot it synced with, kept in `~/.config/obmem/sync/`), local files and the latest snapshot. If one side changed, it wins. If both edited, the local version stays and the remote one becomes `<stem>.sync-conflict-<device>-<time>.<ext>`, which the `sync-conflict` audit rule reports. An edit beats a deletion. The snapshot is created before any local change, so a lost race leaves nothing half-applied. A sync that would delete more than half of the vault needs `--allow-deletes`.
- There is no one-way `backup` command: a push that ignores the other device's changes would drop them from the latest snapshot, and the next sync would delete them there.
- Session log names carry the device (`<date>-<device>`), so two machines never write the same file.
- Paths use `/` and NFC. Names Windows cannot hold are skipped; case collisions are refused. `.obsidian/` uses an allowlist modeled on Obsidian Sync: settings JSON, themes and snippets, never `workspace*.json`, and community plugins only when the user opts in. Names a device does not sync are carried along untouched.

## Alternatives considered
- Obsidian Sync: paid.
- Remotely Save (a community plugin): syncs only while Obsidian is running, and knows nothing about findings or session logs.
- A recovery key in addition to the passphrase: the user declined.
- A conditional `HEAD` pointer: dropped, because create-only snapshot numbers already serialize the writers.

## Consequences
- Session logs from another device stay hidden in the session-start context, because they record another project root. That binding is a security rule: the 2026-11-09 security review found that a repo-chosen project `name` could otherwise reach another project's session logs. Linking logs across devices needs an explicit link the user makes, never an automatic one.
- All four phases were merged on 2026-11-09 (PRs #3 to #6): backup and restore, two-way sync, automation (the hooks sync at session start and in the background at Stop; the GUI has a Sync card), and retention with prune, the only operation that may delete bucket objects.
- The user's own machine has synced to a real bucket since then. Windows is still unverified.
