# AI-004 — Reconcile desktop local profiles on window focus

**Date:** 2026-10-31  
**Implementation SHA:** `c9b2693e1f9b78cf407b271996012aada58cf4aa`  
**Status:** Partial AI-004; parent checkbox remains open.

## Problem and behavior

After optimistic concurrency checks were added, an editor in one desktop window could still display a stale profile list until it attempted a write. That was safe against silent overwrites, but gave no early indication that another window had changed the active profile or saved model.

The AI Settings component now rereads `read_local_ai_profiles` on browser window focus. It validates the response with the existing strict frontend parser and compares the active selection and ordered profile fields. On a change, it refreshes only the saved-profile list and displays a status notice. It does **not** replace the user's unsaved name, protocol, endpoint, selected model, or inventory; it does **not** perform model discovery or inference.

An effect cleanup prevents unmounted/stale listeners from updating state. Per-focus request sequence and editor-revision checks reject responses overtaken by a newer focus or local form edit. During an active save/delete/activate operation, the focus listener is suspended. The native stale-editor precondition and SQLite compare-and-swap remain authoritative for all mutations.

## Evidence

- `desktop/src/AiAssistantSettings.tsx`: focus-triggered, draft-preserving refresh and user notice.
- `desktop/src/aiLocalModels.ts`: exact saved-profile snapshot comparator.
- `desktop/scripts/aiLocalModels.test.mjs`: same collection, active-profile changes, model changes and list changes.
- Exact implementation `c9b2693e1f9b78cf407b271996012aada58cf4aa`:
  - [Rust workspace 41270944482](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41270944482) — passed.
  - [Platform shells 41270944500](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41270944500) — passed (frontend tests/typecheck and native shell).
  - [Desktop launch smoke 41270944487](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41270944487) — passed.

## Separate CI repair

The preceding documentation commit `6a108cb59dc990b92b6651d0b4724fcd27b852b1` failed [Rust workspace 41267165633](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41267165633) because a parallel pack-export test's temporary directory name collided (`AlreadyExists`). The test helper now retries `create_dir` with an atomic per-process nonce. Exact repair `bece4c8fa4c17c4d23b3efa00a5ceacb203d2e7a` passed [Rust workspace 41270574997](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41270574997), [Platform shells 41270575109](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41270575109), and [Android Rust ABI 41270575003](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41270575003).

## Remaining acceptance

This is **focus reconciliation**, not a cross-window push-notification bus. Changes made while a window remains focused are not proactively broadcast. There is no interactive conflict-merge UI, no remote-provider consent, no cloud credential handling, no real inference and no physical-device qualification. Continue AI-004 with these gates unchecked. 
