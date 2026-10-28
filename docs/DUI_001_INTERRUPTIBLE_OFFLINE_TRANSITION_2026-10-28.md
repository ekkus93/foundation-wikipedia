# DUI-001 / SRC-004 — Interruptible offline transition

Date: 2026-10-28. Scope: desktop online text-preview shell, not installed offline packs.

## Problem and change

The Work offline checkbox was disabled whenever a network operation was busy, so the code intended to invalidate pending online responses could not be exercised until the request completed. The desktop reader now keeps that control available while an online search, exact-title/page open or saved-bookmark fetch is pending. Enabling offline mode immediately invalidates the current request epoch, releases the busy UI and clears stale search results. The already loaded revision-pinned, sandboxed text previews and Back/Forward state remain readable but are explicitly not labeled installed or media-complete. Stale network completion callbacks check their epoch before mutating UI state. Reconnection now replaces the obsolete connection-lost notice with an accurate status; it does not silently disable an explicit Work offline choice.

## Evidence and limits

`desktop/scripts/requestEpoch.test.mjs` includes a manual-offline cancellation regression for an in-flight request, a later online request and a second cancellation. The code uses the existing epoch guard for both results and finalizers; the offline checkbox now calls `setBusy(false)` after invalidation. This is deterministic state-machine coverage, not a browser event-injection or live-network race test.

Implementation SHA: `374bac4e868a4a453d376f1719b9c4fe4ea436e7`.

Exact-head CI passed: [Rust workspace 40673392406](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40673392406), [Platform shells 40673392452](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40673392452), [Desktop launch smoke 40673392338](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40673392338).

Remaining: browser-level cancellation timing, installed verified catalog and mandatory offline media, complete source authentication, desktop/Android production integration and physical-device acceptance. **DUI-001 and SRC-004 remain unchecked.**
