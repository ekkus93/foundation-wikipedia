# MOB-002 / MOB-003 / MOB-004 — Android offline and Settings shell (partial)

**Date:** 2026-10-28
**Code commit:** b919f15c3842f06ca3bbee5c3ffd6cdd4c62a0a9
**Scope:** Android Compose presentation and bootstrap WebView only.

## Implementation

- Added a pure Kotlin `ReaderShellState` reducer for the FAB, Settings navigation, Android Back and session-only offline choice.
- The Settings screen exposes a visible Work offline switch. The FAB's Offline action changes the same state; Chat/Bookmark remain unavailable until the application can load an authenticated, verified article.
- `rememberSaveable` retains the offline choice and Settings location through configuration recreation while intentionally collapsing the action menu.
- The bootstrap WebView now explicitly disables network resource loads, mixed content, JavaScript, DOM storage, file/content access and top-level URL navigation. The Android manifest still declares no Internet permission.
- Added `ReaderShellStateTest` JUnit cases for offline preference propagation, Back ordering, Settings navigation and configuration restoration.

## Exact-head evidence

- [Rust workspace 40714093264](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40714093264): passed.
- [Platform shells 40714093442](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40714093442): passed; includes Android `assembleDebug`, `testDebugUnitTest`, `lintDebug` and frontend/native shell checks.
- [Android install smoke 40714093169](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40714093169): still running at documentation time. Do not claim emulator success until it completes.

## Nonclaims and remaining acceptance

The switch does **not** enable live fetching when off or prove that any Wikipedia article is installed when on. The WebView renders only bundled placeholder HTML. No real source provenance, required figures, math, captions, attribution, local-media handler, authenticated pack activation, WebView article link navigation, full-screen media, bookmark/history storage, AI bridge, or reader scroll restoration is qualified. No physical Android device, TalkBack or process-death test was run. MOB-001/002/003/004, MOBPACK and release acceptance tasks remain unchecked.

Next: finish the typed Rust/UniFFI Android bridge, then implement source-authenticated online and verified installed-pack article navigation through the same shared Rust core, with media and WebView origin gates.
