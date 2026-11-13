---
type: decision
title: 'A project vault can be private: outside the repo, named by the user''s config'
status: active
author: agent
confidence: high
created: 2026-11-14
updated: 2026-11-14
verified_commit: cb846ae
code_refs:
- crates/obmem-core/src/config.rs#private_vault_path
- crates/obmem-core/src/config.rs#Config::vault_is_private
- crates/obmem-core/src/config.rs#project_sync
- crates/obmem-core/src/settings.rs#make_private
- crates/obmem-sync/src/run.rs#Target
- crates/obmem-sync/src/files.rs#AppFiles
tags:
- vault
- privacy
- config
- sync
approved_at: 2026-11-14
approved_by: auto-approve
---
## Context
The project vault was always in the repo (`docs/kb`) and travelled with git, so anyone who can read the repo can read the team's memory. On 2026-11-14 the user asked for project memory that a team can keep out of the repo and still share.

## Decision
The user chose three things on 2026-11-14:
- The target is **team sharing**, not only one person's machines.
- **One project has one vault location**: in the repo or private. There is no third scope, and no project with both.
- A private vault is a **standalone vault folder**, not a folder inside the personal vault.

Where the vault is (layer 1, PR #29):
- Scope stays `project` and `global`. Where the project vault is became a separate property. `Store`, audit, drift and the Obsidian calls only see a path.
- The folder is named only by the user's config: `[[project]]` with `root` and `vault` in `~/.config/obmem/config.toml`. `.obmem.toml` can say `private = true`, which names no folder. A machine whose config names no vault for such a project fails `Config::load`, so a teammate who has not set it up never writes private notes into `docs/kb`.
- The mapping is in `config.toml`, not in `projects.toml`: the GUI removes entries from that list, and a removed entry would send writes back into the repo.
- A private vault must be outside the project and apart from the personal vault (`config::private_vault_path`).
- `obmem init --private <folder>` does all of it: starts a private vault, moves a project to one, or tells another machine where its copy is. It moves no notes.
- A private vault keeps every rule the project vault has (exact fingerprints only in its allowlist, `NoteRef::At` for findings): its content comes from teammates, like a clone's.
- Session logs stay in the personal vault, so each person's log stays theirs in a team.

How it travels (layer 2, CHI-105):
- obmem-sync syncs a `Target`: a vault and its bucket. The personal vault is one. A private project vault is another, through `[project.sync]` under its `[[project]]`.
- A project target takes only the bucket from `[project.sync]`. `device`, `auto` and `ephemeral` are the machine's, from `[sync]`. `config::project_sync` merges them into one `SyncConfig`, so the remote, the keychain entry and the state file have one code path.
- A project vault has a backup of its own: the same bucket and prefix as the personal vault is refused, because the two would share a key and a line of snapshots.
- A project vault syncs nothing of `.obsidian/`. Those settings are each person's own.
- A project target's keys come from the keychain only. The `OBMEM_SYNC_*` variables are the personal vault's: with them, a first sync would write the team's bucket under a key no teammate has.
- `obmem sync` syncs every target; a subcommand is about one, chosen with `--vault personal|project`.

## Alternatives considered
- A folder inside the personal vault (`Projects/<name>/`): the personal sync would carry it for free, but it cannot be shared with a team, and Obsidian's vault id lookup would need to change.
- A project with both a repo vault and a private one: a third scope. The user declined.
- Letting `.obmem.toml` name the folder or the bucket: a clone could then send writes outside the repo.
- A separate config type for a project's sync instead of merging into `SyncConfig`: every function of obmem-sync would need a second form.

## Consequences
- Without a `[project.sync]`, a team shares the folder its own way, for example as a private git repository.
- A machine without a keychain (a server, a cloud sandbox) cannot sync a project vault yet. That is part of CHI-106.
- A joining machine must fetch the vault before `init` writes its first files, or `Index.md` exists on both sides and the team's version arrives as a conflict copy. Today that means `obmem sync restore --vault project` into the empty folder first; one command for it is CHI-106.
- A team shares one bucket prefix and one passphrase, with an S3 token per member. One member cannot be cut off from the key alone; that needs a new prefix.
- The merge keeps both versions as a `sync-conflict` copy when two people edit one note, which a team will hit more often than one person's two machines. See [[personal-vault-backup-and-sync-are-built-into-obmem-over-user-provided-s3-r2]].
- Not checked against a real bucket as of 2026-11-14: the project target was tested with the in-memory store and by the CLI's messages, not with `obmem sync setup --vault project` and a keychain.
