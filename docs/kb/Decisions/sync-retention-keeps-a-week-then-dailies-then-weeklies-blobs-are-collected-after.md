---
type: decision
title: Sync retention keeps a week, then dailies, then weeklies; blobs are collected after a 24 h grace
status: active
author: agent
confidence: high
created: 2026-11-09
updated: 2026-11-09
verified_commit: 84661a3
code_refs:
- crates/obmem-sync/src/prune.rs#prune
- crates/obmem-sync/src/remote.rs#Remote::put
tags:
- sync
- backup
approved_at: 2026-11-09
approved_by: auto-approve
---
# Sync retention keeps a week, then dailies, then weeklies; blobs are collected after a 24 h grace

Part of [[personal-vault-backup-and-sync-are-built-into-obmem-over-user-provided-s3-r2]] (phase P4).

## Decision
- The retention rule is fixed, with no configuration. It keeps the latest snapshot, every snapshot of the last 7 days, the newest of each day for 30 days, and the newest of each ISO week for a year. A snapshot's age is its object time in the bucket listing, so choosing what to keep needs no decryption.
- A blob is deleted only when no kept snapshot references it **and** it is older than 24 h.
- Blob uploads changed from create-only to a plain put. A sync uploads every blob its new snapshot needs beyond the latest snapshot's, even when the bucket already has it. Only snapshots and `key.age` stay create-only.
- A prune runs after a successful sync, at most once a day per device, and its failure never fails the sync. `obmem sync prune [--dry-run]` runs it by hand.

## Why
The race with a create-only upload: a sync finds an old blob that no snapshot references, skips the upload, and a concurrent prune then deletes that blob before the sync's snapshot references it. Rewriting the blob renews its object time, so the grace period protects it. Blobs the latest snapshot references are always kept, because the latest snapshot is always kept.

## Alternatives considered
- Configurable retention: speculative; nothing asked for it.
- A cross-device lock for prune: there is no lock primitive in S3/R2 short of conditional writes, and the grace period already covers syncs, which take seconds.
