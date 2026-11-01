# AI-005 / RAG-001 / RAG-002 — Pure revision-scoped evidence preview for consent

**Date:** 2026-11-01  
**Repository:** `ekkus93/foundation-wikipedia`  
**Verified source SHA:** `c31f1acf17e522865f3aa25c447b5c6c5d886ca9`  
**Scope:** Shared Rust `crates/wiki-ai/src/rag.rs`; partial implementation only.

## Motivation

The selected-local-model RAG API requires an affirmative per-request passage-sharing decision, but callers could not inspect the exact bounded source excerpts before making that decision without duplicating the provider prompt-packing logic. A duplicated UI preview could diverge from the bytes ultimately sent, especially when JSON escaping or long questions displace lower-ranked evidence.

## Implementation

Added `prepare_article_context(article, question)`, a pure shared-core function returning `Option<PreparedArticleContext>`. It validates the canonical article and question, prioritizes exact-revision selected evidence, builds the JSON-escaped prompt under the existing 65,536-byte user-message cap, and exposes the exact included evidence and message without invoking a provider or network. A private lexical index is carried alongside the preview for the eventual citation validator. No retrieved evidence returns `None`; invalid or oversized explicit selection fails closed. `answer_article` now reuses the same preparation function before calling `stream_selected`, avoiding separate packing algorithms.

This preview is **not an authorization token or evidence of user consent**. The existing `SelectedLocalModel::answer_article` still requires per-request `share_article_passages=true`; the future desktop/Android UI must disclose endpoint, locality, selected model and exact passages, obtain explicit permission, then revalidate article/revision identity when initiating inference. It must never substitute a text-only online HTML preview for a verified canonical article.

## Verification

- A fake provider captures the exact outbound user message; the pure preview is byte-for-byte identical and contains exactly the same revision-bound selected evidence.
- A long-question, 15-passage fixture confirms the preview stays within the byte cap, retains some fitting passages, and excludes the last-ranked dropped passage; the existing test verifies dropped-passage citation rejection.
- Existing JSON evidence-boundary spoofing, provider rejection, cancellation and citation tests remain part of the Rust workspace suite.

Initial `ff9c1fea5f8c430b64733b2213ebcf3985769e41` failed rustfmt ([Rust 41338676782](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41338676782)); follow-up `aad0711922a7a508ed19b42e08f4a79c0fbc8df1` failed strict Clippy's `len_zero` lint ([Rust 41338899128](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41338899128)). Both were corrected on `master`. Final source `c31f1acf17e522865f3aa25c447b5c6c5d886ca9` passed:

- [Rust workspace — 41339027683](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41339027683)
- [Platform shells — 41339027688](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41339027688)
- [Android Rust ABI qualification — 41339027699](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41339027699)

## Remaining

No desktop/Android chat surface yet loads canonical article evidence, shows the actual model destination and passages, obtains consent, generates and persists validated answers, or navigates to validated citation blocks. Live Ollama/llama-server LAN, factual entailment evaluation, interactive cancellation and real-device acceptance remain outstanding. **AI-005, RAG-001 and RAG-002 remain unchecked.**
