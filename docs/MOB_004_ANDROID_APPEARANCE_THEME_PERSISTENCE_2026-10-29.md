# MOB-004 — Android Appearance Theme Persistence (partial)

**Date:** 2026-10-29 (America/Los_Angeles)  
**Source commit:** `90e6e3000a7aa076d2db22ead6fd33a8b322a5e9`  
**Scope:** Android native Settings appearance only; not full MOB-004 acceptance.

## Implementation

- Added System, Light, and Dark choices to the existing Appearance & Reading Settings destination.
- Applied the selection to the Compose Material 3 color scheme, retaining Android dynamic colors on API 31+ and the existing fallback palettes on earlier versions.
- Persisted the nonsecret theme selection using private versioned `SharedPreferences`; Compose `rememberSaveable` preserves the choice through configuration recreation.
- Missing, malformed, and future preference encodings default to System. A legacy preference of the wrong stored type does not crash app startup.
- Added pure Kotlin JUnit tests for all supported theme encodings and unknown/corrupt inputs.

## Exact-source CI

- [Rust workspace, passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41010632272)
- [Platform shells, passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41010632330), including Android shell build, unit tests, and lint.
- [Android emulator install smoke](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41010632357): running when this report was written; recheck before claiming its result.

## Qualification boundary

The reader remains a hardened local bootstrap WebView, not a verified Wikipedia article. Appearance controls do not establish article/scroll restoration, actual bookmark/history integration, system accessibility or physical-device behavior. `MOB-004` remains unchecked. No branch or pull request was created.
