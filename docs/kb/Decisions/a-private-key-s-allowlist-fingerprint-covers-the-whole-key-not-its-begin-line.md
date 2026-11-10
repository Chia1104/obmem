---
type: decision
title: A private key's allowlist fingerprint covers the whole key, not its BEGIN line
status: active
author: agent
confidence: high
created: 2026-11-10
updated: 2026-11-10
verified_commit: 0ce6fee
code_refs:
- crates/obmem-core/src/secrets.rs#secret_end
- crates/obmem-core/src/secrets.rs#push_hit
tags:
- security
- secrets
approved_at: 2026-11-10
approved_by: auto-approve
---
# A private key's allowlist fingerprint covers the whole key, not its BEGIN line

# A private key's allowlist fingerprint covers the whole key, not its BEGIN line

## Context
A project vault's allowlist may list fingerprints, because a fingerprint is meant to allow one exact secret. The `private-key` rule matches only the `-----BEGIN … PRIVATE KEY-----` line, and the fingerprint was a hash of that match. That line is the same for every key of its type, so anyone could compute it. One entry in a cloned repo's allowlist hid every private key in that project (security finding F1 of 2026-11-09, CHI-69).

## Decision
The hit is still the BEGIN line. Its fingerprint is taken over the span `secret_end` gives: from the BEGIN line through the next END line, or to the end of the text when there is no END line. Redaction already removed exactly that span, so both now share one function.

## Alternatives considered
- Make the rule's regex match the whole block: a key reaches a note in many layouts (quoted, commented, on one line, with escaped newlines, cut off before its END line). The redaction tests list eight. One regex for all of them would miss some, and a miss means no hit at all.
- Never let a fingerprint silence the `private-key` rule: safe, but the fingerprint a blocked write prints would then not work when a person adds it.
- Ignore project-vault fingerprints of low-entropy matches: a heuristic that guards this rule only by accident.

## Consequences
- Allowlisting one key, for example a published test key, allows that key and no other.
- A note that only mentions a BEGIN line, with no END line after it, has a fingerprint that covers the rest of the text. It changes when that text changes. Write such a mention so that it does not match, or add an END line.
- Stopwords and allowlist regexes still see only the BEGIN line. They count only in the personal vault, which is the user's own choice.
- Existing allowlist entries for a BEGIN line stop working. That is intended.
