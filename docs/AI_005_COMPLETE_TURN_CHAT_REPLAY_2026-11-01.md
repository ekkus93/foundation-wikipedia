# AI-005 — Complete-turn bounded chat replay

**Date:** 2026-11-01  
**Scope:** Partial AI-005 / STATE-001; parent acceptance remains unchecked.  
**Verified source SHA:** `74bb9fc11ca7e08ff3ef2b80ce1b3dbd6914a192`

## Problem

The existing `recent_chat_messages(limit)` intentionally returns the newest *N messages*. When `N` is odd, the first row can be an assistant answer without its question. Legacy single-message writes may also leave an orphan question, and malformed role ordering can be persisted outside the supported atomic Q&A path. Passing these fragments as conversation turns to an LLM would misrepresent the conversation.

## Implementation

- Added public `ChatExchange { question, answer }` and `UserState::recent_chat_exchanges(key, revision_id, thread_id, limit)` to `wiki-store::user_state`.
- Reads at most 500 exchanges (1000 messages) through the existing bounded, revision-scoped tail reader, which checks row IDs, roles, message length, and invalid NUL content.
- Requires every selected adjacent pair to be exactly `user` followed by `assistant`; refuses an orphaned or misordered tail rather than inventing a missing question/answer.
- Preserves chronological order and original row IDs. No network access, schema migration, AI invocation, or privacy-policy change occurs.
- This validates the **selected tail**, not every older message in a long legacy thread. It does not independently validate factual entailment or citation authenticity of previously saved answers.

## Automated verification

A deterministic in-memory SQLite regression verifies the latest two complete exchanges out of four, chronological IDs, zero and excessive limits, wrong revision, rejection of an unanswered legacy question, recovery after the matching assistant row is appended, and rejection of a tampered assistant role.

Initial source `709762a05ae0e4fdb8eb3c1f2dd11e7e4d9e6a16` failed Rust formatting only ([Rust 41313257423](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41313257423)); corrected source `74bb9fc11ca7e08ff3ef2b80ce1b3dbd6914a192` passed:

- [Rust workspace — 41313306816](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41313306816)
- [Platform shells — 41313306826](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41313306826)
- [Android Rust ABI qualification — 41313306815](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41313306815)

[Android record codec probe — 41313306810](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41313306810) was still running when this report was written.

## Remaining qualification

The desktop and Android chat surfaces still do not call the revision-scoped RAG or this history reader. A complete integration needs an authenticated canonical Article and evidence, explicit per-request permission before sending article passages, pinned selected-model identity, cancellation, citation validation, privacy-generation-aware atomic persistence, and an accessible citation UI. Legacy orphan rows should be handled with an explicit recovery flow rather than silently reinterpreted. **AI-002, AI-004 and AI-005 remain unchecked.**
