# AI-002 — Model identifier anti-spoof validation

**Date:** 2026-10-31  
**Repository:** `ekkus93/foundation-wikipedia`  
**Qualified source SHA:** `346968e3ac20b6aceba0eb41331f70dc74400d14`

## Scope

A model identifier returned by a user-selected local server or passed to `GenerateRequest` could previously contain Unicode bidirectional controls or invisible formatting characters. Such an identifier can look different from the actual model selected for inference, undermining the provider/model disclosure required by the spec. ASCII control characters, whitespace trimming and size limits were already checked, but these Unicode controls were not.

## Changes

- Centralized model-ID validation in `wiki-ai/src/lib.rs`. The same predicate applies to outbound generation requests, OpenAI-compatible `/v1/models` responses, and native Ollama `/api/tags` responses.
- Reject Unicode bidi controls, isolate/override characters, zero-width format characters, soft hyphen, Unicode line/paragraph separators, BOM, variation selectors and the supplemental variation-selector range. Preserve ordinary non-ASCII letters such as `modèle-1` and standard local model names such as `qwen3:8b` and `org/model:Q4_K_M`.
- Added synthetic tests at all three boundaries. The adapters continue to reject duplicate names, malformed responses and oversized inventories.

## Exact-head qualification

- Initial source `fcdfa6bd0361646b84e17768a738bc85708a2cf7` failed formatting only: [Rust workspace 41245348063](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41245348063).
- Corrected source **`346968e3ac20b6aceba0eb41331f70dc74400d14`**: [Rust workspace 41245413955](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41245413955) **passed**; [Android Rust ABI 41245413943](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41245413943) **passed**; [Platform shells 41245413910](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41245413910) was in progress at documentation.

## Limitations / remaining acceptance

This is an identifier-display integrity guard, not server authentication or model provenance verification. It does not implement a model selection UI, authenticated HTTPS, actual LAN Ollama/llama-server acceptance, grounded answers, or physical-device testing. **AI-002 remains unchecked.**
