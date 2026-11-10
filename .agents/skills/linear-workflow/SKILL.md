---
name: linear-workflow
description: How this repo tracks open work in the Linear project Obmem (issues CHI-n). Load before creating, changing or closing a Linear issue, when opening a pull request for an issue, when a session ends with work left over, or when asked to review or sync Linear.
---

# Linear workflow

Linear holds **open work**: what a person or an agent still has to do. It is one of three records, and each fact goes in one of them:

| Record | Holds |
| --- | --- |
| Linear issue | work still to do, with a "Done when" |
| obmem note | what was decided or learned (the `obmem` skill) |
| session log (`memory_log`) | what this session did and left |

Work that starts and finishes in one session needs no issue.

## Where

- Project **Obmem** (`P-CHI-3`), team **Chia1104**. Issue ids are `CHI-<n>`. Every issue for this repo is in that project.
- Labels: one of `Bug`, `Feature` or `Improvement`. Add `Blocked` when the work waits on something outside the code, such as an upstream release, an account or a machine. The team's other labels belong to another product.
- Statuses: `Backlog`, `Todo`, `In Progress`, `In Review`, `Done`, `Canceled`, `Duplicate`.

## What you may change

**Routine.** Do it, then say what you did:

- move an issue to `In Progress` when you start it, and to `In Review` when its pull request is open
- link a pull request to its issue
- comment with a new fact: what was checked, what was found, the evidence
- correct a description that the code or a release has overtaken, on an issue or on the project

**Ask first.** List the changes and wait for a yes:

- create an issue
- close an issue: `Done`, `Canceled` or `Duplicate`
- change a priority
- anything this list does not name

## Issue shape

Write the description for a person: short sentences, one term per thing.

- **Title:** the action to take. "Deprecate the five unscoped npm packages", not "npm packages".
- **Body:** what is true now, in two to four sentences. Then the steps or the options, when they are known. Then `Done when:` with one sentence a person can check.
- **Agent notes:** file paths, what was already checked and how, commands. Put them last, in a collapsible section that opens with `+++ Agent notes` and closes with `+++` on a line of its own.
- State what you checked. Mark a guess as a guess: a wrong cause in an issue sends the next session the wrong way.
- Search the project for the topic before proposing an issue, so the same work is not tracked twice.

## Pull requests

Put `Fixes CHI-<n>` in the pull request body, one line per issue. Linear then links the pull request to the issue. Write `Part of CHI-<n>` when the pull request leaves the issue unfinished.

After opening it, read the issue. If it does not show the pull request, add the link. If it is not `In Review`, move it.

After the merge, read the issue again. If it is still `In Review`, propose `Done`.

## Sync

Run this when asked to review Linear, and at the end of a session that changed what is open.

1. **List.** Read the project's description and every issue that is not `Done`, `Canceled` or `Duplicate`.
2. **Collect what changed** since those issues were last updated: merged pull requests (`gh pr list --state merged`), releases (`gh release list`), and what the session log says is left.
3. **Settle each open issue.** It is one of:
   - **unchanged**: leave it.
   - **overtaken**: a fact in it is no longer true. Correct the description, or comment with the evidence.
   - **done**: its `Done when` holds. Propose closing it, with the evidence.
   - **unblocked or newly blocked**: check the thing it waits on, such as the newest upstream release. Correct the label and say what changed.
4. **Find untracked work.** A "left" line in the session log, a failing check or a step only the user can take, with no issue: propose one.
5. **Check the project description.** Its facts are the repo, the newest release and "Done so far". Correct what the repo has overtaken.
6. **Report** three lists: changed, proposed and waiting for a yes, checked and unchanged.

The sync is done when every open issue has one of the four outcomes.
