# AI-005 — Consent-gated local-model article RAG bridge

**Date:** 2026-11-01  
**Scope:** Partial AI-002 / AI-004 / AI-005; parent tasks remain unchecked.  
**Verified source SHA:** `c2959a44238e3ffff7fdfdf3ec34942139fbf92d`

## Problem

Model discovery and the fixed diagnostic inference test already used the pinned local adapter, while the revision-scoped RAG pipeline accepted a generic `LlmProvider`. There was no shared entry point binding an explicitly discovered local model to article-grounded generation with a separate per-request passage-sharing decision.

## Implementation

- Added `SelectedLocalModel::answer_article` in `crates/wiki-ai/src/local_selection.rs`. It reuses the selected protocol, exact pinned localhost/private-LAN endpoint and exact model ID, with no provider substitution or cloud fallback.
- Requires an affirmative `share_article_passages` argument for **each invocation**. A saved profile or model discovery is not permission to transmit article excerpts.
- Rejects an `ArticleQuestion` naming a model different from the selected model, before contacting the endpoint.
- Reuses `rag::answer_article` for article/revision-scoped retrieval, bounded evidence, outbound locality policy, cancellation, buffered model text and validation against the exact retrieved citation IDs. Citation validity is provenance validation, **not** factual entailment.

## Automated verification

A deterministic loopback HTTP/SSE test first discovers a native Ollama model and then verifies:

- Missing passage consent and model mismatch do not produce inference requests.
- On-device-only policy blocks a localhost provider, and pre-cancellation blocks inference.
- A model response citing the supplied revision-scoped block returns a supported answer.
- A response citing a fabricated block fails closed.
- Only the two explicitly authorized attempts transmit the article excerpt to the pinned endpoint.

Initial source `f1f33734aa4c3d59a8f64e816130c8debc595e24` introduced the bridge; `c54e9cc929a87f8f663b7577dd68b9f592a0498d` added the loopback test but failed Rust formatting ([run 41303695888](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41303695888)). The corrected exact source `c2959a44238e3ffff7fdfdf3ec34942139fbf92d` passed:

- [Rust workspace — 41303736712](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41303736712)
- [Platform shells — 41303736711](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41303736711)
- [Android Rust ABI qualification — 41303736755](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41303736755)

## Remaining qualification

No desktop or Android chat UI calls this API yet; the UI must show the exact destination, obtain explicit per-request passage-sharing consent, retain article revision identity, and render only validated citations. Chat persistence, streamed UI lifecycle, interactive cancellation, live Ollama/llama-server LAN acceptance, factual grounded-answer evaluation and physical-device acceptance remain open. The consent argument is an API guard, **not** evidence of a consent UX. Do not check AI-002, AI-004 or AI-005.
