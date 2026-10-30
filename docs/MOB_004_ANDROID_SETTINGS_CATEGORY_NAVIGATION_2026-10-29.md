# MOB-004 — Android Settings category navigation (2026-10-29)

## Implementation

The native Compose Settings screen now exposes the nine planned categories:
General, Appearance & Reading, AI Assistant, Wikipedia & Offline Data,
Storage, History & Saved Content, Privacy & Network, Advanced, and About.

- A stable typed category ID is stored with the `rememberSaveable` shell state.
  Restoration accepts an unknown/removed ID by returning to the Settings root.
  Previously saved two-element shell state remains accepted.
- Android Back and the app-bar back control return from a category to the Settings
  root, then from Settings to the reader. The floating action menu stays closed.
- The persisted Work offline switch lives under Wikipedia & Offline Data.
  Other categories show an explicit development-build explanation, not fake
  controls for unimplemented capabilities.
- The Settings content scrolls on small screens.
- Pure Kotlin JUnit tests cover category transitions, Back ordering and
  configuration restoration.

## Qualification and limitations

The exact-head Platform shells CI runs Android compilation, unit tests and lint.
This source-level slice does not yet implement settings for AI, bookmarks,
history, installed packs or storage, nor prove actual article/scroll restoration,
real-device behavior, or offline visual fidelity. The bootstrap WebView remains
network-disabled. **MOB-004 and STATE-003 remain unchecked.**

## Exact-head automated evidence

Implementation SHA `e7a2af6c9b9987f92099e049c98fe628651e6b8c` passed
[Rust workspace CI 41005698864](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41005698864)
and [Platform shells CI 41005698788](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41005698788),
including Android APK compilation, JUnit and lint. The separate
[Android emulator smoke 41005698771](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41005698771)
was still running at this update and must be checked before claiming success.
