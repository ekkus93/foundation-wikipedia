# AI-005 — Distinguish provider rejection from oversized RAG output

**Date:** 2026-11-01  
**Scope:** Partial AI-005; parent task remains unchecked.  
**Verified source SHA:** `882ca421446da3dc377daf7f57d7d481921f2386`

## Problem

`rag::answer_article` previously mapped **every** `ProviderError::Rejected` to `RagError::ResponseTooLarge`, even when the adapter itself rejected an inference request without emitting a single output token. This misreported failures to callers and obscured whether the RAG output limit was reached.

## Implementation

The bounded stream callback now records when its own `MAX_RESPONSE_BYTES` check fails. Only a callback rejection caused by this limit becomes `RagError::ResponseTooLarge`; an independent provider rejection remains `RagError::Provider(ProviderError::Rejected)`. The shared streaming contract still prevents a provider from swallowing callback errors and claiming successful completion.

A deterministic fake-provider test uses the same valid revision-scoped article/question to exercise both a provider-origin rejection and a single oversized text delta. The former retains the provider error; the latter reports the response-size limit. Neither is returned as a supported answer.

## Exact-head CI

All workflows passed for `882ca421446da3dc377daf7f57d7d481921f2386`:

- [Rust workspace — 41304322296](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41304322296)
- [Platform shells — 41304322308](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41304322308)
- [Android Rust ABI qualification — 41304322285](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41304322285)

## Remaining qualification

This is an error-classification fix in the shared RAG layer. It does not add a chat UI, consent UX, real model-server acceptance, persisted history, factual entailment validation, or physical-device evidence. **AI-005 remains unchecked.**
