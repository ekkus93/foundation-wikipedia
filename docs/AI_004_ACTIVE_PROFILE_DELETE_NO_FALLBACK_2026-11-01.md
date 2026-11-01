# AI-004 — Deleting the active profile cannot select a fallback provider

**Date:** 2026-11-01  
**Verified source commit:** `adba05ac93d6b5205ac035d08ca09cc29cd12d15`  
**Status:** Scoped AI-004 improvement. AI-004 remains unchecked.

## Defect

The desktop native `remove_local_ai_profile` handler previously replaced a deleted active profile with the first remaining saved profile. When the remaining profile pointed to a different localhost or private-LAN server/model, deleting one saved configuration silently selected another destination. This contradicted the product's explicit provider-selection and no-silent-fallback policy. Profile selection alone is not permission to send article passages, but it must not change implicitly.

## Correction

`desktop/src-tauri/src/local_ai_profiles.rs` now clears the active selection to `None` when removing the active profile, while retaining other saved profiles for explicit reactivation. Removing an inactive profile leaves the existing active selection unchanged. The existing durable SQLite compare-and-swap and monotonic revision guard remain in place.

`desktop/src/AiAssistantSettings.tsx` now discloses the deactivation consequence in the deletion confirmation, distinguishes active from inactive deletion in its completion notice, and explains that no alternate server/model is automatically selected.

## Automated evidence

The native Rust regression `deleting_active_profile_never_selects_another_saved_destination` verifies durable `active: null` with a remaining LAN profile, rejection of stale-editor activation, explicit reactivation, and unchanged active selection after deleting an unrelated profile. The existing create/switch/update/delete test was updated to require no implicit fallback.

All three exact-source workflows passed:

- [Rust workspace — 41358978466](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41358978466)
- [Platform shells — 41358978449](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41358978449)
- [Desktop launch smoke — 41358978441](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41358978441)

## Limits and next steps

This change prevents silent *saved-profile selection*, not an already-running inference operation or pending network diagnostic. Live Ollama/llama-server LAN interoperability, explicit article-passage consent, canonical evidence loading, answer generation/citation navigation, Android integration, and physical desktop/device acceptance remain outstanding. No article text is sent by deleting a profile. **AI-004/AI-005 remain unchecked.**
