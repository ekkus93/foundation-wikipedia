# AI-004 — Atomic non-secret local-profile updates across desktop processes

**Date:** 2026-10-31  
**Repository:** `ekkus93/foundation-wikipedia`  
**Qualified source SHA:** `7ba9aeb1653557c35419a2ac74dd04e00d577783`  
**Status:** Partial AI-004 concurrency fix; parent task remains unchecked.

## Problem

The new eight-profile manager used a SQLite setting read followed by an unconditional upsert. Two desktop windows performing overlapping profile mutations could each read the same prior collection, then the later write would silently erase the first window's change.

## Implementation

- Shared `wiki-store::user_state::UserState::compare_and_swap_setting(key, expected, value)` enforces key/value bounds and internal-generation-key exclusion identically to ordinary non-secret preferences.
- A **single atomic SQLite INSERT ... SELECT ... ON CONFLICT ... WHERE** checks the exact prior value before changing it, including absence represented by `NULL`. An interleaving writer changes the outcome to false rather than overwriting its value.
- Desktop `local_ai_profiles` upsert/activate/remove now capture the prior persisted v2 setting and use compare-and-swap. Conflicting native read-modify-write operations return a typed-safe failure string rather than clobbering another profile collection.
- The read-only legacy v1 compatibility path is unchanged. Writing v2 and retiring v1 remains safe against concurrent initial migrations because only one absent-value CAS can succeed. Existing bounded profiles, no secret storage and explicit user-initiated discovery policies remain intact.

## Verification

- Initial SHA `a1bf27c4a930bfb6e222906b1c8dd8f5fedaa28c` failed formatting in [Rust workspace 41265977906](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41265977906).
- Corrected source SHA **`7ba9aeb1653557c35419a2ac74dd04e00d577783`** passed [Rust workspace 41266181504](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41266181504), [Platform shells 41266181525](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41266181525) (desktop frontend/native Tauri/Android shell), and [Android Rust ABI 41266181617](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41266181617).
- A separate Android emulator codec probe [41266181509](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41266181509) was running at qualification; it is not evidence of desktop profile concurrency.
- Disk-backed two-handle SQLite tests assert that an outdated expected value, including an outdated absent-key expectation, does not overwrite a newer write; refreshed expected values permit a subsequent update.
- Native profile manager test confirms a stale profile snapshot cannot replace a newer collection/active selection.

## Remaining

- This change rejects **concurrent native read/modify/write conflicts**, but a frontend field value loaded earlier can still be deliberately saved after another window has modified the same profile: native mutation reads a fresh snapshot at operation entry. Full stale-editor protection requires an explicit original version token from the UI plus refresh and conflict resolution.
- No cross-window push notification, physical desktop acceptance, remote/local inference, user consent or hosted-provider secret-store integration is established.
- The existing AI-004 named-profile report's 'last-writer-wins' warning is now narrowed to **stale frontend editor values**, not simultaneous native read/modify/write conflicts.

**AI-002 and AI-004 remain unchecked.**
