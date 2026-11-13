---
type: decision
title: 'The calling agent is the retrieval loop: memory tools say where to search next, and obmem runs no model'
status: active
author: agent
confidence: high
created: 2026-11-14
updated: 2026-11-14
verified_commit: 3fa0335
code_refs:
- crates/obmem-core/src/context.rs#recall
- crates/obmem-core/src/context.rs#unmatched_parts
- crates/obmem-core/src/context.rs#linked
- crates/obmem-core/src/store.rs#Got
- crates/obmem-core/tests/recall.rs#recall_on_this_repos_vault
tags:
- search
- architecture
approved_at: 2026-11-14
approved_by: auto-approve
---
# The calling agent is the retrieval loop: memory tools say where to search next, and obmem runs no model

## Context
Matching in `memory_context` and `memory_search` is literal, so a task in Chinese does not find notes in English, and related notes were reachable only by reading a note and calling `memory_get` once per link. On 2026-11-14 the user asked to improve recall "the Agentic RAG way".

## Decision
The user chose (2026-11-14) to treat the agent that calls obmem as the retrieval loop. obmem stays a set of deterministic tools and runs no model. The tools give the agent what it needs to decide its next call:
- `memory_get` lists the notes a note links to, next to its backlinks.
- `memory_context` lists the terms each match contains, the parts of the task that no note contains, and the notes one link away from the three best matches.

The work is layered. Built on 2026-11-14: a measurement (`crates/obmem-core/tests/recall.rs`) and the signals above. Not built yet: a search routine in the `obmem` skill (search again in other words or the notes' language, follow links, when to stop), and changes to the ranking, which wait until the measurement shows a need.

## Alternatives considered
- A model inside obmem that rewrites queries, reranks or summarises: needs an API key or MCP sampling, makes answers differ from run to run, and the caller is already a model.
- Embeddings and a vector index: would bring back an index, against [[use-the-obsidian-cli-for-search-and-link-checks-instead-of-an-own-index]]. The two vaults held 47 notes when this was decided.
- Extracting a knowledge graph from the notes: the `[[links]]` are already a graph that people review.

## Consequences
- Recall across languages depends on the agent searching a second time. The signal for it is the "no note contains" line.
- A signal belongs in the tool's answer, not only in the skill: an agent does not always follow a routine, and it always reads the answer.
- Measured on 2026-11-14 on this repo's `docs/kb` with the built-in search (28 notes, 8 matches per task): English tasks found 31 of 32 expected notes, 24 of them in the first three, and all 32 counting the linked notes. Chinese tasks found 5 of 9, and 6 counting linked notes. The three Chinese tasks that found nothing all named what no note contains.
- The vault is small, so "found" is easy. Read "in the first three" when judging a ranking change.
- Both backends gave the same links and backlinks for all 28 notes of `docs/kb` (Obsidian 1.14.5, 2026-11-14). See [[obsidian-cli-behaviour]] for the `links` command.
