---
type: concept
title: obmem index
status: active
author: human
---
# obmem

> Map of content. Loaded into every agent session — keep it short and current.

## Architecture
- [[split-obmem-into-a-cargo-workspace-so-a-gui-can-reuse-the-core]]: which crate owns what, and why the GUI is a workspace member.
- [[use-the-obsidian-cli-for-search-and-link-checks-instead-of-an-own-index]]: search and link checks come from Obsidian. There is no index.
- [[a-headless-backend-answers-for-obsidian-where-the-app-is-not-installed]]: the built-in search for machines without Obsidian.
- [[the-calling-agent-is-the-retrieval-loop-memory-tools-say-where-to-search-next-an]]: how recall improves without a model or an index, and how it is measured.

## Key decisions
- [[agent-guidance-is-a-short-always-loaded-guide-plus-skills-that-setup-installs]]: what agents are always told, and what is in the skills.
- [[detect-unlogged-work-from-git-for-agents-other-than-claude-code]]: how the stop hook decides that a session did work.
- [[personal-feedback-can-be-for-one-project-by-a-tag-and-claude-code-imports-arrive]]: feedback for one project (`project/<name>` tag), and what a Claude Code import does.
- [[obmem-gui-scope-control-panel-for-macos-and-windows-notes-stay-in-obsidian]]: what the desktop app does and does not do.
- [[the-desktop-app-translates-in-the-window-and-libraries-report-typed-values-with-]]: the desktop app's two languages, and why libraries report typed values.
- [[personal-vault-backup-and-sync-are-built-into-obmem-over-user-provided-s3-r2]]: encrypted sync of the personal vault.
- [[sync-retention-keeps-a-week-then-dailies-then-weeklies-blobs-are-collected-after]]: what prune keeps and deletes.
- [[remote-machines-sync-with-credentials-from-the-environment-and-an-ephemeral-sett]]: sync on servers and in cloud sandboxes.
- [[a-project-vault-can-be-private-outside-the-repo-named-by-the-user-s-config]]: a project vault outside the repo, how a team syncs it and how a machine joins.
- [[obmem-update-replaces-only-standalone-installs-by-running-the-release-s-install-]]: `obmem update` and the install channels.
- [[npm-packages-are-published-under-the-chia1104-scope]]: why the npm name has a scope.
- [[the-secret-scanner-never-looks-up-a-value-from-an-env-file-on-disk]]: how the scanner tells a path from a secret.
- [[a-private-key-s-allowlist-fingerprint-covers-the-whole-key-not-its-begin-line]]: what an allowlist fingerprint of a private key allows.

## Conventions & gotchas
- [[obsidian-cli-behaviour]]: read before changing how obmem calls the Obsidian CLI.
- [[obmem-in-a-git-worktree-cannot-reach-obsidian]]: memory search fails in a worktree on a machine with Obsidian.
- [[t3-code-s-cursor-driver-starts-mcp-servers-in-the-home-directory]]: why tools take a `project` argument.
- [[cursor-hook-payloads-and-agent-transcript-format-seen-2026-11-09]]: what Cursor sends to hooks.
- [[token-shaped-test-fixtures-must-be-split-with-concat]]: GitHub rejects a push with a token-shaped literal.
- [[rustfmt-check-exits-0-on-stdin-even-when-it-prints-a-diff]]: why the pre-commit hook compares hashes.
- [[target-grows-because-p-obmem-gui-and-workspace-build-native-shell-with-different]]: prefer `--workspace` to `-p obmem-gui`.
- [[a-new-npm-package-name-answers-404-for-minutes-after-its-first-publish]]: wait before calling a first publish failed.

## Runbooks
- [[releasing-a-version-bump-last-then-tag-the-merged-commit]]: the order of merge, bump and tag.
