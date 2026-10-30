# MOB-004 — Retain the Android reader WebView across Settings (2026-10-29)

## Problem and change

The initial Compose shell rendered either Settings or the reader `AndroidView`.
Opening Settings removed the WebView from composition; returning created a new
WebView and reloaded bootstrap HTML. When real article rendering arrives, this
would discard the active document and scroll position.

The shell now always mounts the same WebView inside a Compose Box and overlays
Settings as an opaque native screen. While Settings is visible, the WebView is
`INVISIBLE` and excluded from accessibility descendants. Returning to the
reader restores its visibility without invoking the WebView factory or reloading
the document. The Settings overlay uses the full Scaffold content area and
remains scrollable on small screens.

## Verification and remaining work

The exact-head Android build/unit/lint and emulator launch gates must be
checked separately. This is a source-level lifetime improvement, not a
real-device scroll-position or process-death restoration test. The WebView
continues to block JavaScript, network resource loading, file/content access
and top-level URL navigation. No authenticated article, verified offline
media, installed pack, or in-WebView citation bridge is claimed.

**MOB-002 and MOB-004 remain unchecked.**
