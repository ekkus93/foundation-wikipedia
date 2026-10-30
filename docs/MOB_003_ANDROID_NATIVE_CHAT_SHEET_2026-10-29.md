# MOB-003 — Native Android article-chat bottom sheet (partial)

**Date:** 2026-10-29 (America/Los_Angeles)
**Base master:** `3a33d6ddfeda6033222bae0cb0c1781eca8c6295`
**Scope:** Android Compose shell interaction only; this is not a working AI assistant.

## Implementation

- Chat speed-dial action opens a native Material 3 modal bottom sheet instead of a transient snackbar.
- The sheet explicitly states that a verified article, revision-scoped citations, and an explicitly configured AI provider are required. It does not request a model or transmit article/user data.
- Android Back, outside-tap dismissal and Close chat dismiss the sheet. The WebView remains mounted and its accessibility descendants are hidden while the modal is open.
- Transient chat is not restored after recreation. Settings and Chat are mutually exclusive; the offline preference is preserved.
- Pure Kotlin state regressions cover opening, dismissal, Back, Settings exclusion, offline preservation and restoration.

## Verification boundary

Check exact-source Platform shells CI for Android compilation, unit tests and lint after the commit. State tests do not prove actual modal gestures, TalkBack focus, article restoration or device behavior. No verified article or AI service is wired up. **MOB-003 remains unchecked.**
