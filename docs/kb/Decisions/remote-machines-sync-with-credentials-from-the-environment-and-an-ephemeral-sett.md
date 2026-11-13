---
type: decision
title: Remote machines sync with credentials from the environment and an ephemeral setting
status: active
author: agent
confidence: high
created: 2026-11-10
updated: 2026-11-10
verified_commit: 38a6b4e
code_refs:
- crates/obmem-sync/src/keychain.rs#from_env
- crates/obmem-sync/src/run.rs#create_new_vault
- crates/obmem/src/main.rs#sync_and_wait
- crates/obmem-core/src/config.rs#SyncConfig
tags:
- sync
- remote
- architecture
approved_at: 2026-11-10
approved_by: auto-approve
verified_at: 2026-11-10
verified_by: agent
---
# Remote machines sync with credentials from the environment and an ephemeral setting

## Context
Remote agents run in cloud sandboxes: a new Linux machine per session, with no OS keychain, no personal vault and no state from an earlier session. obmem's sync read its credentials only from the keychain, and its hooks left the sync in a background process that such a machine can outlive.

## Decision
- A machine without a keychain takes the bucket's access key and the vault key from three environment variables (`keychain::ENV_VARS`). `obmem sync key` prints the vault key on a machine that is already set up.
- `[sync] ephemeral = true` marks a machine that does not outlive the session. The session-start hook then syncs before it reads the context, the stop hook waits for its sync, the first sync creates the vault folder, and the machine never prunes.

## Alternatives considered
- Put the backup passphrase in the environment instead of the vault key: no export command would be needed, but every sync would pay the scrypt unlock, and the passphrase is a secret a person memorises and may use elsewhere.
- Infer "ephemeral" from the credentials coming from the environment: a long-lived server without a keychain also uses the environment, and it should keep background syncs and pruning.
- `[sync]` values from environment variables: not needed. `OBMEM_CONFIG` and a config file written by the sandbox's setup script already do it.
- A fixed `device` name for sandboxes: sandboxes that run at the same time would write the same session log and get conflict copies. Leaving `device` unset gives each its host name.

## Consequences
- The vault key in a cloud secret store reads the whole personal vault, and what an agent writes there syncs to every machine. A read-only or partial vault for the cloud would need a second key and prefix, and is not built.
- On an ephemeral machine every turn end waits for a sync.
- A vault folder cached in a sandbox image must be cached together with `~/.obmem/sync/`. A vault without its sync state is merged as if both sides had changed, which makes conflict copies.
- Checked end to end on 2026-11-10 against a local mock S3 server (moto): a machine with no vault fetched it in the session-start hook, its stop hook uploaded a new file, and the first machine received it.
