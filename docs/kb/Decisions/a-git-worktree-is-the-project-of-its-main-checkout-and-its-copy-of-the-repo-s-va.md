---
type: decision
title: A git worktree is the project of its main checkout, and its copy of the repo's vault is read from the files
status: active
author: agent
confidence: high
created: 2026-11-09
updated: 2026-11-14
verified_commit: dd55720
code_refs:
- crates/obmem-core/src/config.rs#main_checkout
- crates/obmem-core/src/obsidian.rs#Backend::for_project_vault
- crates/obmem-core/src/sessions.rs#own_roots
tags:
- worktree
- obsidian
- config
- privacy
approved_at: 2026-11-14
approved_by: auto-approve
verified_at: 2026-11-09
verified_by: agent
---
## Context
A linked git worktree (`git worktree add`, T3 Code `isolation: worktree`, Claude Code's agent worktrees) is the same project at another path. Until 2026-11-14 obmem took it for another project, and memory search failed in it:
- A private vault was not found: `[[project]]` in the user's config names the main checkout's path. With `.obmem.toml` in the worktree, `Config::load` failed. Without it, notes went to the worktree's `docs/kb`, inside the repo.
- Session logs split: a log's header records the project root, so the worktree started its own log and did not see where the last session stopped.
- Every worktree was added to the list of projects, and one without a `name` in `.obmem.toml` took its folder's name, so feedback for the project was not shown.
- A vault in the repo is a copy at a path Obsidian has never opened. On a machine with the Obsidian CLI, `memory_search`, `memory_context`, links and backlinks reported "not an Obsidian vault yet", and the audit skipped `broken-link` and `orphan`.

## Decision
The user chose both layers on 2026-11-14.

**The project is its main checkout.** `Config` has two roots:
- `project_root` is the folder with the files: `.obmem.toml`, a vault in the repo, the env files, the code.
- `main_root` is the repo's main checkout (`config::main_checkout`). What is kept outside the project's folder is under it: `[[project]]` and `[project.sync]`, `projects.toml`, the root in a session log's header, the default project name, Claude Code's memory folder for an import.

A function that takes a root and reads one of those calls `main_checkout` itself, so a caller passes the root it has.

**`main_checkout` asks the repo, not the folder.** It runs `git worktree list --porcelain -z` and takes the first entry only when the folder is in that list. The list comes from the repo's own `.git/worktrees/`. It runs git only where `.git` is a file: a main checkout has a folder, so the usual case costs nothing.

**A worktree's copy of the repo's vault is read by `Headless`** (`Backend::for_project_vault`). Each vault of a store has its own backend. On a machine with Obsidian they differ in this one case, decided when the store opens from where the vault is. The global vault, a private vault and the main checkout's vault stay with Obsidian, and so does a copy that someone did open in Obsidian.

## Alternatives considered
- `git rev-parse --git-common-dir` in the `[[project]]` lookup only (proposed in another session). Tested on 2026-11-14 with git 2.54: a folder whose `.git` file is one line, `gitdir: <another repo>/.git`, gets that repo's common dir. Any downloaded folder could then name a private project's repo and be given its vault in the session-start context. It also left the session logs, the project list and the name as they were.
- Mapping a vault in the repo to the main checkout's vault, which Obsidian knows. The worktree's notes are the branch's: they differ from the main checkout's, and writes must merge with the branch.
- Leaving search degraded in worktrees and running agents in the main checkout. That was the advice before; agents in worktrees are the common case now.
- Registering each worktree's vault in Obsidian. Obsidian cannot register a vault from the command line, see [[obsidian-cli-behaviour]].

## Consequences
- In a worktree, a project note is searched with the built-in search, which supports part of Obsidian's syntax. See [[a-headless-backend-answers-for-obsidian-where-the-app-is-not-installed]]. `obmem status` says so, and names the main checkout (`worktree of:`).
- `obmem init --private <folder>` in a worktree records the main checkout in `[[project]]` and marks the branch's `.obmem.toml`.
- Logs that a worktree started before this change record the worktree's path. No session lists them now; nothing migrates them.
- A worktree of a bare repository is the project of the repository's folder, the first entry git lists.
- A repo whose `.mcp.json` and hook files are not committed has no obmem in a worktree at all. That is the repo's to fix, as chia1104.dev showed.
- With git older than 2.36 (`-z` is missing) a worktree is its own project, as before.
