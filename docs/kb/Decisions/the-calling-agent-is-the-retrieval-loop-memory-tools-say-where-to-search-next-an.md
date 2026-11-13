---
type: decision
title: 'The calling agent is the retrieval loop: memory tools say where to search next, and obmem runs no model'
status: active
author: agent
confidence: high
created: 2026-11-14
updated: 2026-11-14
verified_commit: cbb3670
code_refs:
- crates/obmem-core/src/context.rs#recall
- crates/obmem-core/src/context.rs#unmatched_parts
- crates/obmem-core/src/context.rs#linked
- crates/obmem-core/src/context.rs#count
- crates/obmem-core/src/context.rs#weight
- crates/obmem-core/src/context.rs#share
- crates/obmem-core/src/store.rs#Got
- crates/obmem-core/tests/recall.rs#recall_on_this_repos_vault
tags:
- search
- architecture
approved_at: 2026-11-14
approved_by: auto-approve
---
## Context
Matching in `memory_context` and `memory_search` is literal, so a task in Chinese does not find notes in English, and related notes were reachable only by reading a note and calling `memory_get` once per link. On 2026-11-14 the user asked to improve recall "the Agentic RAG way".

## Decision
The user chose (2026-11-14) to treat the agent that calls obmem as the retrieval loop. obmem stays a set of deterministic tools and runs no model. The tools give the agent what it needs to decide its next call:
- `memory_get` lists the notes a note links to, next to its backlinks.
- `memory_context` lists the terms each match contains, the parts of the task that no note contains, and the notes one link away from the three best matches.

The work is layered, and each layer is judged by a measurement (`crates/obmem-core/tests/recall.rs`):
1. The measurement and the signals above (2026-11-14).
2. The ranking (2026-11-14, CHI-109). A word counts only where a word starts, so `rag` is not in `storage`. A term is worth more the fewer of the found notes have it, so `does` and `not` decide little. A note gets more for a term it has often, and a long note needs it more often (the term-frequency part of BM25). `Index.md` is named in one line and is never a match.
3. The routine (2026-11-14, CHI-108). The Recall section of the `obmem` skill is five steps: start with `memory_context`, read its signals, search again in other words or in the notes' language, read and follow links, and stop when nothing new is named.

## Alternatives considered
- A model inside obmem that rewrites queries, reranks or summarises: needs an API key or MCP sampling, makes answers differ from run to run, and the caller is already a model.
- Embeddings and a vector index: would bring back an index, against [[use-the-obsidian-cli-for-search-and-link-checks-instead-of-an-own-index]]. The two vaults held 47 notes when this was decided.
- Extracting a knowledge graph from the notes: the `[[links]]` are already a graph that people review.
- For the ranking, a longer list of common words: it is one list per language and never complete. The weight by rarity needs no list.
- For the ranking, counting a word only as a whole word: `token` would no longer find `tokens`.
- A recall subagent that `obmem setup` installs, to run the routine in its own context: not built. The routine took 5 to 7 calls and under 20 seconds in the tests below, which the main agent can afford, and only Claude Code has such subagents.

## Consequences
- Recall across languages depends on the agent searching a second time. The signal for it is the "no note contains" line.
- A signal belongs in the tool's answer, not only in the skill: an agent does not always follow a routine, and it always reads the answer.
- `memory_context` sends its terms to the search as bare words. Obsidian finds a quoted word almost only as a whole word and the built-in search finds it anywhere, so quoted terms gave the two backends different notes to rank. See [[obsidian-cli-behaviour]].
- The ranking's numbers (rarity, average length) are counted from the notes one search returned. There is still no index.
- Measured on this repo's `docs/kb` with the built-in search (about 30 notes, 8 matches per task), expected notes found and found in the first three by one `memory_context` call:

| Tasks | Layer 1 | Layer 2 |
| --- | --- | --- |
| 19 in English, 32 notes | 31 found, 24 in the first three | 31 found, 26 in the first three |
| 7 questions and tasks with a short word, 7 notes | 7 found, 5 in the first three | 7 found, 6 in the first three |
| 6 in Chinese, 9 notes | 5 found, 4 in the first three | 5 found, 4 in the first three |

- The vault is small, so "found" is easy. Read "in the first three" when judging a ranking change.
- The three Chinese tasks that found nothing in one call all named what no note contains. On 2026-11-14 a Claude Sonnet subagent that followed the skill's routine through the CLI found the expected note for each of the three, with its second call. The measurement cannot check this: it needs an agent.
- An agent with no note title to go by cannot tell which language the notes are in. In the tests the subagents guessed English. The skill points to the session's start context and to `[[Index]]` for it.
- Checked on 2026-11-14 with Obsidian 1.14.5: both backends gave the same links and backlinks for the 28 notes `docs/kb` then held, and after layer 2 the same `memory_context` answer for all 32 tasks of the measurement, with this machine's personal vault included.
