# AI-005 / RAG-002 — Bounded JSON-isolated article RAG prompt

**Date:** 2026-11-01  
**Scope:** Partial AI-005 / RAG-002. Parent tasks remain unchecked.  
**Qualified source SHA:** `432c499bfdf71db5d35178c89efc026f2da971b3`

## Problem

The revision-scoped Rust RAG pipeline previously inserted a question, heading paths and article excerpts into a newline-delimited plain-text prompt. An untrusted excerpt containing `\nEvidence ID:` or `\nHeadings:` could impersonate a new prompt record, even though final citation validation would still reject fabricated block IDs. The context byte limit also excluded the question and outbound framing.

## Change

- JSON-encode the question as a string, each heading path as an array, and each excerpt as a string before embedding them into the outbound provider message. A newline in a passage or question becomes the literal `\\n` inside a JSON string instead of a structural line break.
- Keep actual evidence IDs as trusted, revision-scoped record delimiters, using the existing validated article index and retrieval results.
- Check the complete outbound user message against the existing 65,536-byte budget after escaping and framing, before any provider invocation. No extra provider, network destination or fallback is introduced.
- Add a fake-provider test that captures the actual outbound prompt, includes forged `Evidence ID`/heading labels in a passage and question, and verifies only the real selected block is represented as a record and accepted as a citation.

## Verification

Initial source `cc060b265fb4b607c934d6702ac69718dc9965e1` required test-fixture correction; `4246b84395c3b24e69ac53794fae64e54b4c9094` failed rustfmt only ([Rust run 41328084876](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41328084876)). Corrected exact source `432c499bfdf71db5d35178c89efc026f2da971b3` passed:

- [Rust workspace — 41328137711](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41328137711)
- [Platform shells — 41328137807](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41328137807)
- [Android Rust ABI — 41328137829](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41328137829)

## Limitations and next work

JSON escaping prevents a passage from creating additional *syntactic* prompt records. It does not prevent semantic prompt injection, guarantee factual entailment, or establish that an LLM will follow system instructions. Existing citation validation proves that cited IDs were supplied from the exact revision, not that each claim is factually supported. No desktop or Android chat inference UI uses this core path yet. The next high-value integration remains canonical article evidence acquisition, explicit per-request passage-sharing consent, selected-provider disclosure, cancellable generation, and validated citation rendering. Live Ollama/llama-server LAN and physical-device acceptance are still outstanding.

**Do not check AI-005 or RAG-002.**
