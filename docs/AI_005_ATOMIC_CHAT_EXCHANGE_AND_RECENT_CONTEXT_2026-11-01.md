# AI-005 — Atomic revision-scoped chat exchange and recent history

**Date:** 2026-11-01  
**Scope:** Partial AI-005 / STATE-001; parent acceptance remains unchecked.  
**Verified source SHA:** `64002ae59c50ffbe0920f86dff03265db5c1c8bc`

## Problem

The private SQLite user-state store already supported article/revision-scoped chat threads and individual user/assistant messages. Persisting a completed question and its citation-validated answer as two separate inserts could leave a half-turn after an error, cancellation, or a concurrent privacy reset. The existing `chat_messages(limit)` reader returns the *earliest* N rows, not the most recent bounded context needed to resume a long conversation.

## Implementation

- `UserState::append_chat_exchange_for_generation` validates both nonempty UTF-8 message bodies (64 KiB maximum each, no NUL), exact article/project/page/revision/thread identity, and the captured personal-data generation.
- An **IMMEDIATE** SQLite transaction serializes the generation check and both inserts with concurrent writers or privacy-data deletion. Both rows commit together or neither does. Stale-generation operations return `None` without writing; invalid thread/revision and invalid message bodies fail closed.
- This is deliberately a **persistence hook after** RAG citation validation. The store does not assert factual entailment or independently authenticate a caller-supplied answer.
- `UserState::recent_chat_messages` selects the newest bounded N messages and returns them in ascending ID order, preserving conversational chronology. It applies the same role/content validation as the existing reader and rejects cross-revision thread access or limits over 1000.

## Regression evidence

Deterministic in-memory tests exercise invalid assistant bodies and revision mismatches without partial writes, atomic user/assistant ordering, post-clear stale generation rejection, and new-thread isolation. A disk-backed two-handle test proves that a reset from another SQLite connection prevents stale exchange insertion while allowing a new-generation exchange. Tail-reader tests cover ordered last-three retrieval, zero limits, cross-revision rejection, excessive limits, and a corrupted persisted role.

Initial implementation `68be894b5a59dfc503dc51f69492045d3db04eb8` and follow-up `afd56a9d683ff453ac8f5eb549da4d3d3d150480` failed rustfmt-only checks ([Rust 41308098360](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41308098360), [Rust 41308349492](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41308349492)). Formatting was corrected at `51e50d76c84206c8aac9e2442d8c2888d9961356` and `64002ae59c50ffbe0920f86dff03265db5c1c8bc`.

The final exact source SHA passed:

- [Rust workspace — 41308402741](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41308402741)
- [Platform shells — 41308402823](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41308402823)
- [Android Rust ABI qualification — 41308402736](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41308402736)

[Android record codec probe — 41308402733](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41308402733) was still running when this report was written.

## Remaining qualification

The desktop/Android chat surfaces still do not invoke the shared `SelectedLocalModel::answer_article` API or this persistence hook. UI must first establish a **canonical exact-revision Article** (the current online reader exposes a sanitized HTML preview, not automatically trustworthy RAG evidence), obtain explicit per-request passage-sharing consent, bind a selected model to its pinned endpoint, cancel stale requests, validate citations, and only then persist a completed exchange. No chat text is sent to a provider by this change; no cloud fallback or device acceptance is claimed. **AI-002, AI-004 and AI-005 remain unchecked.**
