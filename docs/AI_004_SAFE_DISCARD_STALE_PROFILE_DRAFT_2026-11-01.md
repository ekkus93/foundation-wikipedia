# AI-004 — Explicitly discard stale desktop AI profile drafts

**Date:** 2026-11-01  
**Status:** Partial AI-004; parent remains unchecked.  
**Implementation:** `1f75f69fc6b853a7d4217baf3be243c8d6cf1cb4`

## Problem and behavior

Cross-window saved-profile reconciliation already preserved unsaved fields and blocked Save until explicit acknowledgement, but the only direct resolution was to authorize overwriting the newer saved profile. A user who preferred the other window's changes had no dedicated safe discard/reload action.

The conflict alert now offers **Discard my draft and load latest saved values**, alongside the existing explicit acknowledgement option. The new action chooses the currently saved version of the draft's named profile when present, otherwise the active saved profile; when no saved profiles remain, it resets to an unconnected blank-name editor. It discards unsaved edits only on explicit click. It invalidates pending model-discovery and profile-read responses, clears the review fence, and restores any model ID as an unverified hint; it does not contact a model server or activate a profile in native SQLite. Concurrent changes after the last read are still protected by revision-aware compare-and-swap and later reconciliation.

## Tests and exact-commit CI

`desktop/scripts/aiLocalModels.test.mjs` adds deterministic coverage for a modified named profile, deleted draft with active fallback, and an empty saved collection. The existing desktop build and smoke suites exercise TypeScript compilation and the complete frontend test command.

- [Rust workspace 41290713052](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41290713052): passed.
- [Platform shells 41290712999](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41290712999): passed.
- [Desktop launch smoke 41290713015](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41290713015): passed.

## Remaining acceptance

Real interactive cross-process desktop conflict testing, live Ollama/llama-server LAN acceptance, remote-provider secret storage and explicit consent, article-grounded inference with verified citations, and desktop/Android device acceptance remain unqualified. This is a deterministic editor-resolution improvement, not completion of AI-002/AI-003/AI-004/AI-005.
