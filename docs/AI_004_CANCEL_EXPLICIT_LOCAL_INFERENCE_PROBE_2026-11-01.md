# AI-004 — Cancel explicit local inference diagnostics

**Date:** 2026-11-01  
**Status:** Partial AI-002/AI-004. Neither parent task is complete.  
**Verified source SHA:** `0f82a290dfc0d0f14bcbc970a7bb5d19206e563d`

## Problem

The first explicit **Test selected model** implementation invalidated stale UI results when Work offline was enabled or the user changed a model. It did **not** signal the already-running native HTTP streaming request to stop. A test could continue sending and receiving its fixed diagnostic after the user withdrew local-network permission.

## Implementation

- `LocalAiProbeRegistry` in the desktop Tauri adapter owns bounded, single-use diagnostic leases. Preparing a lease is a local IPC operation with **no network traffic**. Each lease contains a shared `CancellationToken`.
- The frontend first prepares a lease, verifies its current request generation and offline status, then passes the lease ID to the native test command. A late prepare response is cancelled rather than used. Native test invocation must atomically claim a still-active lease; a cancelled lease cannot be restarted or claimed twice.
- Offline transitions, component unmount, protocol/endpoint/model edits, and profile changes cancel the active lease and invalidate stale UI responses. The native cancellation command is idempotent.
- The native probe checks cancellation before discovery and after discovery, preventing a cancelled inventory GET from proceeding to an inference POST. The existing shared streaming transport observes cancellation during bounded 250 ms socket reads and rejects any subsequently completed response.
- At most 32 leases can be registered; abandoned **unstarted** leases are reclaimed after five minutes. A worker removes its lease on completion, including a worker join failure.
- The diagnostic still sends only `Reply OK.` with an eight-token output limit to the explicitly selected pinned localhost/private-LAN model. No article text, chat history, secrets, or model output crosses the diagnostic IPC boundary; no cloud fallback exists.

## Automated verification

The corrected source commit passed all required exact-head workflows:

- [Rust workspace — 41294770616](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41294770616) — passed.
- [Platform shells — 41294770632](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41294770632) — passed, including desktop native Rust and frontend checks.
- [Desktop launch smoke — 41294770665](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41294770665) — passed.

New native regression tests exercise lease registration/claim, refusal of a second claim, cancellation visible to an in-flight token, refusal to restart a cancelled lease, idempotent cancellation, pre-cancelled probe refusal before any socket opens, 32-lease capacity and expired-lease reclamation. Existing shared HTTP transport tests cover cancellation during streaming.

The first source commit `07171bb1f7bd483eaced8f85896f7a786c21b83a` failed desktop native compilation because `tauri::Manager` was not imported. `164e1ac82016366b136336c91857ea917c7008b2` fixed that import and passed the full workflow set. The final source commit above additionally hardened lease cleanup and its tests.

## Remaining qualification

The model-discovery transport is a bounded **blocking** HTTP GET that does not currently accept a cancellation token. If offline is enabled during that GET, the GET may finish or time out before the native test returns; its post-discovery cancellation check prevents an inference POST. This is not evidence of live Ollama/llama-server LAN compatibility, interactive cancellation timing on a real desktop, article-passage consent, verified article-grounded answers, remote credential storage, or Android acceptance. **AI-002, AI-004 and AI-005 remain unchecked.**
