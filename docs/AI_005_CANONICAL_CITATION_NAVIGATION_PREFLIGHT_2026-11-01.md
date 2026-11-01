# AI-005 / RAG-002 — Canonical citation navigation preflight

**Date:** 2026-11-01  
**Repository:** `ekkus93/foundation-wikipedia`  
**Verified source SHA:** `e691f6fa6e33e29294d80039eb20c45b495e018f`  
**Status:** Partial implementation; AI-005 and RAG-002 remain unchecked.

## Problem and implementation

The existing `answer_display_parts` checks the model's citation markup against a caller-supplied list of typed `EvidenceHit` records. It deliberately cannot prove that a hit still belongs to the currently displayed canonical article. An eventual clickable-citation adapter needs the exact article project/page/revision and must reject a citation when the reader has changed article, revision, or evidence content.

Added `wiki_ai::rag::answer_navigation_parts(article, answer)`, `ArticleAnswerDisplayPart`, `CitationNavigationTarget`, and `ArticleAnswerDisplayError`. The function first validates the supplied article and builds its revision-scoped lexical evidence index. It then checks the full citation marker grammar and ordered hit correspondence through the existing splitter, and requires **every** cited hit to match the current article's canonical block ID, revision, excerpt, and heading path. Only after those checks does it return literal text spans and typed citation targets carrying the article key, revision ID, and matching evidence. An abstention yields no navigation targets.

**Trust boundary:** A `GroundedAnswer` is publicly constructible, and an arbitrary caller could supply a genuine article hit. This helper validates **article consistency**, not model provenance, affirmative passage-sharing consent, publisher authenticity, or factual entailment. Use it only with freshly validated results of the consent-gated RAG pipeline. Historical saved answers remain unverified plain text and must never be upgraded to trusted links by parsing their citation strings. The UI must also compare the target's article identity to the currently displayed article before navigating.

## Regression and CI evidence

The deterministic Rust regression exercises a valid exact-revision citation, literal text, abstention, a changed revision, changed canonical passage, forged excerpt, forged heading path, mismatched citation marker, and invalid article identity.

The initial code SHA `f41db2a5fadc8d78414b732e1f0ffebcedb7d607` failed only the rustfmt gate ([Rust workspace 41348782659](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41348782659)). The corrected source SHA `e691f6fa6e33e29294d80039eb20c45b495e018f` passed:

- [Rust workspace — 41348841527](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41348841527)
- [Platform shells — 41348841526](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41348841526)
- [Android Rust ABI qualification — 41348841504](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41348841504)

## Remaining

Desktop Chat is still read-only; its online HTML preview is not canonical evidence. Next connect verified installed/normalized article data, show exact evidence/model destination, obtain per-request passage-sharing consent, invoke cancellable local inference, persist only validated answers, and wire typed citation targets to exact reader blocks. Android integration, factual entailment evaluation, live Ollama/llama-server LAN acceptance, and real-device qualification remain open.
