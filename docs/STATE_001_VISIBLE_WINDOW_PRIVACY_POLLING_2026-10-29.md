# STATE-001 — Visible-window cross-process privacy reconciliation (partial)

**Date:** 2026-10-29 (America/Los_Angeles)  
**Repository:** `ekkus93/foundation-wikipedia`  
**Exact implementation SHA:** `f9390b6b4c92b1f743da121d4f3647da473e8a27`  
**Status:** Partial; STATE-001 remains unchecked.

## Problem and implementation

The desktop previously checked persistent SQLite privacy generation only on initial mount, focus and visibility restoration. An unfocused but still visible window could retain stale article previews, bookmark identities, and visit-history state indefinitely after another process cleared personal data or revoked history consent.

- `desktop/src/App.tsx` now performs a five-second generation check while the document is visible. Focus and visibility restoration still trigger immediate checks. The interval is cleared on component teardown and concurrent SQLite checks are suppressed.
- `desktop/src/readerPrivacyGeneration.ts` treats a lower canonical generation as `stale` rather than resetting the last observed generation. SQLite generation is compared as `BigInt`, not rounded JavaScript `number`.
- `desktop/src-tauri/src/lib.rs` ignores a lower or equal durable generation in `reconcile_preview_cache_generation`, preventing a delayed native worker from rolling back the last seen epoch or needlessly clearing newer previews.
- `desktop/scripts/readerPrivacyGeneration.test.mjs` and native `privacy_cache_generation_tests` cover reordered reads, unchanged epochs, future valid changes, and signed 64-bit upper-bound handling.

## Exact-head CI

- [Rust workspace (passed)](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40988305702)
- [Platform shells (passed; frontend and native adapter)](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40988305744)
- [Desktop launch smoke (passed)](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40988305755)

These are source and CI qualifications. No physical desktop or Android device was operated.

## Limitations and remaining acceptance

Polling is eventual (five seconds while visible) and is **not** cross-process push notification. A background/hidden window reconciles when made visible. Application-local privacy operations may also advance the durable generation and cause conservative re-reconciliation; precise origin attribution and selective cache invalidation are not implemented. A reset after the last read check but before the next poll cannot be prevented by polling alone. Remaining STATE-001 work includes complete application-adapter/Android integration, lifecycle and restore qualification, cross-process push/notification if required, and physical-device privacy tests. Do not mark the parent task complete on this evidence alone.
