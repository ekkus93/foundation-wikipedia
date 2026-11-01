# AI-004 — Require explicit review before overwriting cross-window profile changes

**Date:** 2026-10-31  
**Source commit:** `a66ce104e77b52d44533116e21a1ca78187e0cc7`  
**Status:** Partial AI-004; the parent remains unchecked.

## Failure mode

The native desktop local-profile mutations already require a validated previously loaded collection and use atomic SQLite compare-and-swap. However, the frontend's focus/native-event reconciliation could replace its `expectedProfiles` with a newer on-disk collection while retaining an older unsaved form. Clicking Save afterward would submit the old form with the new expected collection and could overwrite another window's changes without an explicit acknowledgement.

## Change

- `desktop/src/AiAssistantSettings.tsx` now sets a sticky review requirement when focus/visibility/native-event reconciliation detects a changed saved collection or a failed mutation triggers recovery refresh.
- The Save action is disabled while review is pending; its handler also rejects the operation if invoked despite the disabled UI.
- The editor shows the current saved active profile and, when available, the persisted endpoint and model for the draft's profile name. The user's unsaved form is not discarded or auto-applied.
- An explicit **I reviewed the saved state — allow saving my draft** action clears the review fence. Successful explicit activation, deletion or save also clears it.
- `desktop/src/aiLocalModels.ts` exposes a sticky pure review predicate; `desktop/scripts/aiLocalModels.test.mjs` verifies a changed collection requires review, subsequent identical refreshes cannot clear it, and unchanged initial state does not require review.
- This remains a local SQLite UI flow: no discovery, inference or outbound request occurs on refresh or acknowledgement.

## Exact-head CI

Implementation SHA `a66ce104e77b52d44533116e21a1ca78187e0cc7`:

- [Rust workspace 41278482238](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41278482238): passed.
- [Platform shells 41278482225](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41278482225): passed (frontend tests/typecheck and platform checks).
- [Desktop launch smoke 41278482232](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41278482232): passed.

## Remaining acceptance

This is an explicit overwrite acknowledgement, not a three-way merge or persistent monotonic revision token: an ABA collection cycle is still possible. Multi-window interactive testing on actual desktop installations, remote-provider credentials/consent, local/LAN real inference, article-grounded citations and physical Android/desktop acceptance remain unqualified. AI-002 and AI-004 stay unchecked.
