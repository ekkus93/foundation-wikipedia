# AI-004 — Cancel user-triggered desktop model discovery

**Date:** 2026-11-01  
**Scope:** Partial AI-002 / AI-004; parent tasks remain unchecked.  
**Verified source SHA:** `7dff7a6498ebf609f8c27d6ce9c168187f1b8a5d`

## Problem

The earlier cancellation change covered native model re-discovery within **Test selected model**, but the desktop's standalone **Discover models** action still used an uncancellable blocking GET. An offline switch or endpoint edit discarded stale frontend results without actively stopping the native read.

## Implementation

- Desktop model discovery now requires a native single-use request lease before it can start the network GET. A late lease registration is explicitly cancelled instead of being allowed to open a socket after the UI has changed.
- Native Tauri `discover_local_ai_models` claims that lease, uses the shared cancellable local-model selection API and releases the lease on completion or worker failure. The existing pinned local socket address, protocol, inventory validation and no-cloud-fallback policy remain unchanged.
- Work offline, endpoint/protocol/profile changes, Settings teardown and other selection invalidations cancel an active discovery lease and invalidate its UI response.
- Discovery and inference-test leases live in **separate registries**, preventing a discovery request ID from being used as authorization for a fixed-prompt inference test.
- A new native test checks lease isolation, cancellation propagation and no re-claim after cancellation. Existing transport tests verify a real loopback GET stalled mid-read is interrupted by cancellation.

## Exact-head CI

All three source workflows passed:

- [Rust workspace — 41299997248](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41299997248)
- [Platform shells — 41299997235](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41299997235), including desktop native and frontend checks
- [Desktop launch smoke — 41299997210](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41299997210)

## Remaining qualification

This is native/automated cancellation wiring, not an interactive measured desktop cancellation test against a real model server. Blocking TCP connect and small HTTP writes still use bounded socket timeouts; stalled response reads poll cancellation every 250 ms. No article passages are transmitted by discovery. Live Ollama/llama-server LAN acceptance, explicit article-passage consent, grounded chat/citation UI, hosted provider credentials and physical-device testing remain outstanding. **AI-002, AI-004 and AI-005 remain unchecked.**
