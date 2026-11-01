# AI-004 — Explicitly disable the active local AI provider without deleting saved profiles

**Date:** 2026-11-01  
**Status:** Partial AI-004; parent remains unchecked.  
**Verified implementation:** `fd8b92700b6dd297da7626af8f002f8dcf5cb4b1`

## Behavior

Desktop Settings now has a **Disable active local profile** action. It atomically sets the persisted `active` profile to `null` while retaining all named local profiles for future explicit reactivation. Disabling clears the editor's discovered model selection but preserves editable endpoint/protocol/name and saved model hint. It makes no outbound request and does not uninstall a model. The saved-profile dropdown represents the actual persisted active name, not an unsaved editor draft; when disabled, it shows the empty selection so the user can choose a saved profile to reactivate.

The native `deactivate_local_ai_profile` command requires the exact editor snapshot and uses the existing monotonic revision and SQLite compare-and-swap. A stale window cannot silently disable or reactivate a newer selection. Only successful durable mutations broadcast the payload-free local profile change event. Rust and TypeScript validators permit `active: null` with a nonempty profile collection, while still rejecting nonexistent active names and malformed or secret-bearing profile data.

## Tests and CI

Native Rust regression covers disable, persisted profile retention, revision increment, rejection of stale reactivation, explicit reactivation, repeat disable rejection, and removal while disabled. Node regression covers inactive saved collection parsing, absent active destination disclosure, retained model hints and cross-window conflict detection.

The first implementation SHA `4702296302518a99e3dceeebaebe4dc382df76b0` failed [desktop smoke 41292101858](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41292101858) because an older test still considered inactive nonempty collections corrupt. The fixture was corrected and source `342f3bdd1218e0de32627887950d2ec9a6b662c4` passed [Rust workspace 41292371487](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41292371487), [Platform shells 41292371514](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41292371514), and [Desktop launch smoke 41292371521](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41292371521).

A follow-up fixed the dropdown to reflect the persisted active name rather than the editable draft. Exact final source `fd8b92700b6dd297da7626af8f002f8dcf5cb4b1` passed:

- [Rust workspace 41292646424](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41292646424)
- [Platform shells 41292646426](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41292646426)
- [Desktop launch smoke 41292646433](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41292646433)

## Remaining acceptance

This is a durable provider-profile disable operation, not live inference cancellation or a real desktop acceptance test. AI-002/AI-004 still need live Ollama/llama-server LAN acceptance, explicit passage-consent policy, cloud-provider credentials and consent, actual article-grounded inference with verified citations, and physical-device testing. AI-004 stays unchecked.
