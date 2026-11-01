# AI-005 / RAG-002 — Consent-gated display-ready local inference

**Date:** 2026-11-01  
**Repository:** `ekkus93/foundation-wikipedia`  
**Qualified source SHA:** `72c887f6002f4eb4dab871a3705bba2f0a168595`  
**Status:** Partial; AI-005 and RAG-002 remain unchecked.

## Motivation and implementation

The shared core previously exposed consent-gated local inference as a public `GroundedAnswer` plus separate citation-splitting and canonical navigation helpers. A future UI could accidentally parse historical or caller-constructed answers into trusted links rather than applying the complete chain to a **fresh** inference.

Added `SelectedLocalModel::answer_article_with_preview_display`, returning `LocalArticleDisplayAnswer::{Supported, InsufficientEvidence}` and a typed `LocalArticleError::Display` on navigation-validation failure. It first invokes the existing exact-preview, explicit-passage-consent, selected-local-model, cancellable inference and revision-scoped citation-validation path. Only on a successful supported result does it invoke `rag::answer_navigation_parts` against the current canonical article. The returned supported parts are literal text spans and citation targets carrying project/page/revision/evidence identity. A model abstention remains a distinct state, not an empty supported answer.

The API does **not** stream unvalidated text to the UI, create cloud fallbacks, or accept saved historical answer text as an input. It does not prove factual entailment or publisher authenticity. The caller must display the selected destination and exact evidence and obtain affirmative per-request consent; the UI must also compare the current displayed article identity when handling a citation navigation action.

## Verification

Extended the existing deterministic loopback Ollama integration test. It now supplies discovery plus three HTTP/SSE responses: a valid answer for the existing RAG call, a valid answer for the display-ready call, and a fabricated-citation answer for the rejection path. The test checks that denied consent creates no additional network POST, that successful inference yields literal text and an exact article/revision-bound navigation target, and that the forged citation is rejected.

Exact source SHA `72c887f6002f4eb4dab871a3705bba2f0a168595` passed:

- [Rust workspace — 41349353232](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41349353232)
- [Platform shells — 41349353376](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41349353376)
- [Android Rust ABI qualification — 41349353335](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41349353335)

## Remaining

The desktop chat is still read-only and its online HTML preview cannot serve as canonical revision-scoped evidence. Next wire canonical article loading, exact passage/model disclosure and consent, cancellable inference, persistence of validated answers, and clickable verified block navigation. Android UI integration, real Ollama/llama-server LAN acceptance, factual-entailment evaluation and physical-device testing remain outstanding.
