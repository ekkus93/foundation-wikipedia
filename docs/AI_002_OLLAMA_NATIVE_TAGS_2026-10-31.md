# AI-002 — Explicit native Ollama model discovery and local streaming

**Date:** 2026-10-31  
**Source SHA:** `d60aae83b234fee60060e3e7fc28f4e876dbd241`  
**Scope:** Partial AI-002; local provider adapter only.

## Implemented

The new `wiki_ai::ollama::LocalOllamaProvider` is an explicit alternative to `LocalOpenAiProvider`. It discovers available Ollama models using the native `GET /api/tags` response, while reusing the already bounded and cancellation-aware OpenAI-compatible `POST /v1/chat/completions` streaming transport for inference. Both routes use the **same** caller-approved `LocalEndpoint` and its pinned socket address. A public hostname, metadata IP, redirect, proxy or DNS alias cannot be silently classified as local. HTTPS is rejected until authenticated TLS is available; there is no downgrade or remote fallback.

The native inventory requires a bounded array of at most 256 uniquely named models. It rejects missing, empty, whitespace-padded, overlong, control-containing and duplicate names, and mismatched optional `model` aliases. `/api/tags` does not provide a reliable model context window, so `ModelInfo.context_window` is `None`. The existing OpenAI-compatible model discovery and llama-server path are unchanged.

## Regression evidence

Synthetic loopback TCP tests cover a valid Ollama `/api/tags` response and its exact HTTP request route, malformed/duplicate/mismatched names, a size-bounded inventory, a selected-model chat request producing typed streamed text and completion events, and public-host/HTTPS rejection. These tests do not contact an external model or service.

- Initial `6964db1a925ddcb4560a3852a3cdb1bc1d20d9bc`: [Rust CI 41240682077](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41240682077) failed rustfmt.
- Corrected `d60aae83b234fee60060e3e7fc28f4e876dbd241`: [Rust workspace 41240753700](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41240753700) **passed**; [Android Rust ABI 41240753636](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41240753636) **passed**; [Platform shells 41240753668](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41240753668) was running when this report was authored. Check its final exact-head status separately.

## Remaining acceptance

This is a transport adapter, not a working UI provider-selection flow or grounded article Q&A. Real Ollama and llama-server compatibility, LAN versus on-device labeling in both platform shells, consent, selected-model persistence, authenticated HTTPS, and real-device testing remain unqualified. Keep **AI-002 unchecked** until its end-to-end acceptance evidence exists.
