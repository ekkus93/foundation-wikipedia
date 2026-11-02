# AI-005 / STATE-001 — Consented local inference with atomic chat persistence

**Date:** 2026-11-01 (America/Los_Angeles)  
**Repository:** `ekkus93/foundation-wikipedia`  
**Verified implementation SHA:** `36e5da467f2416de6569520eac056b86e751d828`  
**Status:** Partial. AI-005, STATE-001 and RAG-002 remain unchecked.

## Problem

The shared Rust core already supported preview-bound, consent-gated local RAG inference with validated citation navigation targets, and the private SQLite store supported atomic Q&A appends to an *existing* thread. No shared use case combined them. A UI could accidentally persist unvalidated output or create an empty conversation before inference, leaving orphaned threads when inference failed or consent was revoked.

## Implementation

1. **Atomic new-thread and complete exchange:** `wiki-store::UserState::save_chat_exchange_for_generation` accepts an optional exact-revision thread ID. It checks identity, 64 KiB message bounds and the captured persistent privacy generation inside an IMMEDIATE SQLite transaction. If no thread is supplied, the thread and both messages are created within the same transaction. Invalid messages, wrong-thread/revision identities and stale generations write nothing. The previous `append_chat_exchange_for_generation` API delegates to the same transaction with a required existing thread.
2. **One shared inference-to-persistence use case:** `wiki-ffi::article_chat::answer_and_save_local_article` takes a typed `ArticleChatRequest`, mutable private `UserState` and captured generation. The request carries a canonical exact-revision `Article`, an explicitly discovered/selected `SelectedLocalModel`, the exact `LocalArticlePreview`, the question, affirmative per-request passage-sharing consent, optional existing thread and cancellation token.
3. **No-network preflight:** Missing consent, pre-cancellation, stale personal-data generation and wrong existing thread are rejected before contacting the model. The shared `SelectedLocalModel::answer_article_with_preview_display` revalidates the exact article/question/evidence/model/endpoint/policy against the preview, buffers output until citation validation succeeds and checks navigation targets against the canonical article.
4. **Persist only complete supported answers:** A supported result's validated text and citation markers are reconstructed for *historical plain-text storage*, with a 64 KiB bound. An insufficient-evidence response creates no conversation; a cancellation or privacy reset before the commit prevents persistence. The returned `SavedArticleChatOutcome::Supported` carries fresh typed navigation parts and the durable thread/question/answer row IDs. Historical saved citations are never reinterpreted as trusted links.

The public use case is blocking and requires an off-UI-thread adapter. It does **not** fetch an Article from the current desktop online HTML preview: that preview is not a canonical evidence source. The caller must disclose the exact model/endpoint and previewed passages, obtain affirmative consent for this request, and invalidate the operation if the displayed article changes. The store alone cannot authenticate arbitrary model output or prove factual entailment.

## Automated evidence

- `wiki-store` regression: invalid answer does not create a thread; a successful new thread and both messages are atomic; wrong revision fails without changing the existing conversation; stale generation after a privacy reset creates nothing; fresh generation can write.
- `wiki-ffi` regression: cancellation and insufficient evidence do not create threads; successful supported answer returns typed parts and stores exact citation-marked text; a wrong thread and stale privacy generation fail without partial messages.
- Deterministic loopback Ollama integration: GET discovery selects `qwen3:8b`, a denied-consent request causes no POST or saved thread, and a consented POST/SSE produces a citation-validated supported answer and exactly one persisted complete exchange. No external model or Wikimedia network access is required.

**Exact implementation SHA:** [`36e5da4`](https://github.com/ekkus93/foundation-wikipedia/commit/36e5da467f2416de6569520eac056b86e751d828).

**CI (all passed):**
- [Rust workspace — 41369630959](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41369630959) (format, strict Clippy, workspace tests).
- [Platform shells — 41369630999](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41369630999).
- [Android Rust ABI qualification — 41369630952](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41369630952).

Initial source SHA `387b25f759efa2393c2c23d92ec46708383e956d` failed rustfmt; the corrected SQLite implementation `2995f811de4cacd3b2385f60c6517e2c807b537e` passed Rust workspace. Early `wiki-ffi` revisions required rustfmt, a test-source reconstruction fix and a strict-Clippy argument-count refactor. Only the final implementation SHA above is qualified.

## Remaining work

- Wire a publisher/provenance-checked, exact-revision canonical Article loader into the desktop and Android adapters; do not promote the existing sanitized online HTML preview into trusted RAG evidence.
- Display exact selected endpoint, model and evidence before an affirmative, per-request consent action; run inference on blocking workers with request cancellation and article-identity fencing.
- Render the fresh typed citation navigation parts as safe, clickable, exact-revision article block anchors. Saved history stays unverified until independently revalidated.
- Provide streaming UI without displaying unvalidated citations, integrate selection context and bounded conversation continuation, and test real Ollama/llama-server LAN endpoints.
- Complete end-to-end desktop/Android acceptance, factual-entailment evaluation, offline visual integrity and physical-device testing.

**No top-level acceptance checkbox is qualified by this slice.**
