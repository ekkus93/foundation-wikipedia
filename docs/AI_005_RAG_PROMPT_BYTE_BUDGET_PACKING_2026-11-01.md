# AI-005 / RAG-001 / RAG-002 — Byte-budgeted ranked evidence packing

**Date:** 2026-11-01  
**Repository:** `ekkus93/foundation-wikipedia`  
**Exact source SHA:** `c02d63fa7b43cf58afcd94ed6e0be772e9ade383`  
**Scope:** shared Rust `crates/wiki-ai/src/rag.rs`; synthetic regression only.

## Problem

The article-grounded inference path previously selected passages under a word budget, then built the JSON-escaped provider message. A long but valid question could push the combined message over 65,536 bytes, rejecting the entire request even when fewer passages would fit. Word counts cannot account for UTF-8 bytes, JSON escaping, or evidence-record overhead. A later citation must not refer to a passage omitted from the actual request.

## Implementation

1. Reserve the JSON-encoded question, canonical project/page/revision identity and framing before selecting evidence.
2. Preserve JSON-string isolation for untrusted headings, excerpts and questions.
3. Place exact-revision explicitly selected evidence first; reject invalid or oversized selected passages rather than silently omitting them.
4. Traverse deterministic ranked lexical hits and skip a passage when its full serialized record would exceed the remaining byte budget. Smaller later passages may still fit. The original passage-count and word budgets also apply.
5. Return insufficient evidence when retrieval yields none; report context too large when byte limits prevent including any otherwise eligible passage.
6. Validate claimed citations only against the final list of passages actually sent to the model, not the full index or untrimmed retrieval list. Preserve cancellation and the outbound locality policy.

## Deterministic tests

A synthetic article contains fifteen matching ~3.9 KiB passages. A valid long question makes the full combined prompt too large while a subset still fits. A fake local provider checks that the outbound message stays at most 65,536 bytes and contains at least one but fewer than fifteen authentic evidence records. Explicit abstention is accepted. A citation to the final-ranked, byte-budget-dropped passage is rejected as `UnretrievedCitation`. When that final-ranked passage is explicitly selected with a passage limit of one, it remains included and can be cited. Existing malformed-query, CJK word-budget, provider-rejection and evidence-boundary spoofing tests remain covered.

Initial code `ebfcaed0c3a18b83b9ca76ae605c5b1bd8815a7a` mistakenly used literal backslash-n delimiters; the correction `d096cd024a43eb3cc035f7caf382c64aea236c7f` passed [Rust workspace CI 41333858263](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41333858263) and [Android Rust ABI CI 41333858256](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41333858256). The final regression extension is `c02d63fa7b43cf58afcd94ed6e0be772e9ade383`.

**Exact-final-source CI:** [Rust workspace 41334075261](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41334075261) — passed; [Platform shells 41334075194](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41334075194) — pending; [Android Rust ABI qualification 41334075229](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41334075229) — passed.

## Remaining limitations

This does not implement interactive desktop/Android chat. The desktop's current online preview contains revision-pinned sanitized HTML, not the canonical independently verified article evidence needed for RAG. Do not treat it as source-grounded AI context. Explicit passage-sharing consent, selected-provider disclosure, live Ollama/llama-server qualification, persistence of newly validated answers, clickable exact-block citations and real-device acceptance remain open. Structural citation validation establishes supplied revision provenance, **not factual entailment**. AI-005, RAG-001 and RAG-002 remain unchecked.
