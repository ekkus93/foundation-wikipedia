# AI-005 / RAG-002 — Typed citation display parts

**Date:** 2026-11-01  
**Repository:** `ekkus93/foundation-wikipedia`  
**Qualified source SHA:** `28050741ef5859e28c58b5d2235cb13ce8016b43`  
**Status:** Partial implementation; AI-005 and RAG-002 remain open.

## Motivation

The RAG pipeline returns a citation-validated `GroundedAnswer::Supported` containing raw generated text and a separate exact-evidence citation list. A future desktop/Android renderer must not turn arbitrary `[[cite:...]]` strings or saved historical answers directly into trusted clickable links. It needs a typed, fail-closed representation that distinguishes literal text from citations and keeps the exact revision-bound evidence attached.

## Implementation

Added `wiki_ai::rag::answer_display_parts` with `AnswerDisplayPart::{Text, Citation}` and `AnswerDisplayError`. The function first checks the entire citation-marker grammar, requires at least one citation for a supported answer, and requires a one-to-one ordered match between every marker and the answer's validated `EvidenceHit` list. Duplicate, missing, forged, reordered and malformed markers are rejected. The returned text parts remain literal strings, including Unicode and HTML-looking content; a UI must render them as text nodes, never via `innerHTML`.

`GroundedAnswer::InsufficientEvidence` maps to an empty part list and must be shown as a separate abstention state by the UI.

**Trust boundary:** The `GroundedAnswer` enum is publicly constructible. This splitter checks *consistency*, not article authenticity or whether a model actually received the evidence. Only use it on fresh results of the canonical `answer_article` / consent-gated `answer_article_with_preview` validation path. Do not use it to elevate historical persisted text into verified links.

## Verification

Two new deterministic Rust unit tests cover literal Unicode and HTML-looking strings, two exact-revision citation parts, abstention, and malformed, missing, fabricated, duplicate and reordered citation claims.

The first code commit `e760aab8029a661abfa048ada39ee5db4bf57aac` failed the Rust formatter gate ([Rust 41344015450](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41344015450)); the corrected exact source SHA `28050741ef5859e28c58b5d2235cb13ce8016b43` passed:

- [Rust workspace — 41344078490](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41344078490)
- [Platform shells — 41344078439](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41344078439)
- [Android Rust ABI qualification — 41344078446](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41344078446)

## Outstanding

Desktop chat remains read-only because the current online HTML preview is not canonical revision-scoped evidence. Next wire verified article data and exact evidence preview to the desktop, disclose selected model and endpoint, obtain affirmative per-request passage consent, invoke cancellable inference, persist only validated answers and map typed citation references to verified block navigation. Android integration, factual-entailment evaluation, real local-model LAN acceptance and physical-device testing remain open.
