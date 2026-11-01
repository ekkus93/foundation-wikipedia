# AI-004 — Visible cross-process local-profile reconciliation

**Date:** 2026-11-01  
**Status:** Partial AI-004; parent remains unchecked.  
**Scope:** Desktop saved local-model profile metadata only.

## Failure mode

A Tauri profile-change event reaches windows in the same application process, and focus/visibility restoration refreshes from SQLite. Two independent desktop processes can remain side by side and visible without a focus change. The nonfocused editor could display a stale saved-profile revision indefinitely until another event or focus transition, although native compare-and-swap correctly prevents a stale write.

## Implementation

- `desktop/src/AiAssistantSettings.tsx` refreshes from private SQLite at a five-second interval only while the document is visible and the profile editor is not performing a mutation. Existing focus, visibility and payload-free native event refreshes remain.
- `AiProfileRefreshGate` coalesces simultaneous focus/event/timer triggers into one follow-up read, preventing overlapping native reads and starvation of the latest request epoch on slow disks.
- The existing monotonic revision comparison detects ABA as well as changed profile values. A detected change retains the draft, sets the sticky review fence and requires explicit acknowledgement before Save. All refreshes remain read-only; no model endpoint is contacted.
- The interval is cleared and outstanding requests invalidated on effect teardown; hidden windows perform no periodic reads.
- `desktop/scripts/aiLocalModels.test.mjs` covers the visibility policy and coalescing transitions. The existing snapshot/revision tests cover changed-profile review behavior.

## Verification

**Verified implementation SHA:** `41bf1d83b5332c498505dfe8a3a7c2b45415bfc9` (exact `master` at implementation verification).

- [Rust workspace 41282430915](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41282430915): passed.
- [Platform shells 41282430913](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41282430913): passed, including desktop frontend typecheck, Node tests, native Tauri adapter tests and Android shell checks.
- [Desktop launch smoke 41282430905](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41282430905): passed.

These automated checks do not constitute interactive cross-process desktop acceptance.

## Remaining acceptance

Manual simultaneous-window tests on supported desktop OSes, remote-provider credential storage and explicit consent, actual article-grounded inference and citations, real Ollama/llama-server LAN acceptance, and Android/device validation remain outstanding. AI-002, AI-003 and AI-004 remain unchecked.
