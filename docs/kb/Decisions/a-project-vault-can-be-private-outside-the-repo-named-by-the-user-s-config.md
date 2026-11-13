---
type: decision
title: 'A project vault can be private: outside the repo, named by the user''s config'
status: active
author: agent
confidence: high
created: 2026-11-14
updated: 2026-11-14
verified_commit: f7990d9
code_refs:
- crates/obmem-core/src/config.rs#private_vault_path
- crates/obmem-core/src/config.rs#Config::vault_is_private
- crates/obmem-core/src/settings.rs#make_private
- crates/obmem-core/src/init.rs#project_file
tags:
- vault
- privacy
- config
- sync
approved_at: 2026-11-14
approved_by: auto-approve
---
# A project vault can be private: outside the repo, named by the user's config

## Context
The project vault was always in the repo (`docs/kb`) and travelled with git, so anyone who can read the repo can read the team's memory. On 2026-11-14 the user asked for project memory that a team can keep out of the repo and still share.

## Decision
The user chose three things on 2026-11-14:
- The target is **team sharing**, not only one person's machines.
- **One project has one vault location**: in the repo or private. There is no third scope, and no project with both.
- A private vault is a **standalone vault folder**, not a folder inside the personal vault.

What follows from that, as built in layer 1:
- Scope stays `project` and `global`. Where the project vault is became a separate property. `Store`, audit, drift and the Obsidian calls only see a path.
- The folder is named only by the user's config: `[[project]]` with `root` and `vault` in `~/.config/obmem/config.toml`. `.obmem.toml` can say `private = true`, which names no folder. A machine whose config names no vault for such a project fails `Config::load`, so a teammate who has not set it up never writes private notes into `docs/kb`.
- The mapping is in `config.toml`, not in `projects.toml`: the GUI removes entries from that list, and a removed entry would send writes back into the repo.
- A private vault must be outside the project and apart from the personal vault (`config::private_vault_path`).
- `obmem init --private <folder>` does all of it: starts a private vault, moves a project to one, or tells another machine where its copy is. It moves no notes.
- A private vault keeps every rule the project vault has (exact fingerprints only in its allowlist, `NoteRef::At` for findings): its content comes from teammates, like a clone's.
- Session logs stay in the personal vault, so each person's log stays theirs in a team.

## Alternatives considered
- A folder inside the personal vault (`Projects/<name>/`): the personal sync would carry it for free, but it cannot be shared with a team, and Obsidian's vault id lookup would need to change.
- A project with both a repo vault and a private one: a third scope. The user declined.
- Letting `.obmem.toml` name the folder: a clone could then send writes outside the repo.

## Consequences
- obmem does not carry a private vault between machines yet. Until layer 2, a team shares the folder its own way, for example as a private git repository.
- Layer 2 (not built): obmem-sync takes a sync target instead of the personal vault alone (`run` calls `config::global_vault()` today). A team vault must not sync `.obsidian/` settings, which are per person. A joining machine must fetch the vault before `init` writes its skeleton, or `Index.md` conflicts with the team's.
- Layer 3 (not built): a team shares one bucket prefix and one passphrase, with an S3 token per member. One member cannot be cut off from the key alone; that needs a new prefix.
- The merge keeps both versions as a `sync-conflict` copy when two people edit one note, which a team will hit more often than one person's two machines. See [[personal-vault-backup-and-sync-are-built-into-obmem-over-user-provided-s3-r2]].
