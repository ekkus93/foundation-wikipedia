# MOB-003 — Android Reader Action Menu Outside-Tap Dismissal (partial)

**Date:** 2026-10-29 (America/Los_Angeles)  
**Source commit:** `8a1319961bf191e38f6f95e621a27bcd31581d84`  
**Scope:** Compose reader action menu interaction, not full MOB-003 acceptance.

## Implementation

- When the reader FAB menu is expanded, a transparent Compose overlay intercepts taps outside the action buttons and dismisses the menu.
- The overlay is in the Scaffold content layer, below the FAB/action buttons; it does not reload or unmount the hardened reader WebView.
- Expanded actions hide the underlying WebView from accessibility traversal until dismissal, retaining visibility of the reader.
- Added `ReaderShellState.dismissActions()` and pure Kotlin regression tests verifying dismissal preserves offline mode and Settings state.

## Exact-source CI

- [Rust workspace, passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41011407104)
- [Platform shells, passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41011406899), including Android unit tests, build and lint.
- [Android emulator install smoke](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41011406822): running when this report was written; recheck before claiming its result.

## Qualification boundary

Automated Kotlin state tests and compilation do not prove real touch routing or TalkBack focus behavior. The reader remains a development placeholder; the Chat bottom sheet, verified citations, and physical-device accessibility acceptance are still absent. `MOB-003` remains unchecked. No branch or pull request was created.
