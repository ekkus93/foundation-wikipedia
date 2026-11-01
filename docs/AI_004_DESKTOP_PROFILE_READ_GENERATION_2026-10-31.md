# AI-004 — One generation fence for desktop profile reads

**Date:** 2026-10-31  
**Implementation:** `a1bd81af4cbfe3c51ac95ca02547b92ecd833f16`  
**Status:** Partial; AI-004 remains unchecked.

## Race addressed

The initial saved-profile load, the focus/native-event reconciliation effect, and the conflict-recovery read previously used separate sequencing checks. An old initial load could resolve after a newer native-event refresh and overwrite the saved list or restore an outdated endpoint/model into the editor. A stale failed-mutation refresh could likewise race a later focus read.

All three read paths now use one `RequestEpoch` instance. Each read starts a new generation; only the latest generation may apply its response. Form invalidation, explicit profile saves, effect cleanup and component unmount invalidate outstanding reads. The existing edit-revision check independently preserves user drafts. Failed reads cannot erase the stored list. No profile read triggers discovery or inference.

## Evidence

- `desktop/src/AiAssistantSettings.tsx`: shared generation across initial load, native event/focus/visibility refresh, and failed-mutation refresh.
- `desktop/scripts/requestEpoch.test.mjs`: deterministic initial/event/conflict overlap and explicit invalidation test.
- Exact code SHA `a1bd81af4cbfe3c51ac95ca02547b92ecd833f16`:
  - [Rust workspace 41275212608](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41275212608) — passed.
  - [Platform shells 41275212647](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41275212647) — passed.
  - [Desktop launch smoke 41275212552](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41275212552) — passed.

The automated test proves the generation primitive's ordering, not a real two-window interaction. Interactive event delivery, independent desktop processes, native ABA-proof revision tokens, remote-provider consent, real inference and physical-device acceptance remain unqualified.
