# MOB-004 — Android reading-history consent preference (partial)

**Date:** 2026-10-30 (America/Los_Angeles)  
**Base master:** `74e1546a065c88697aa3ff3183a8096a6cd7abdf`  
**Scope:** Android native Settings privacy preference; no article or history persistence is enabled.

## Implementation

- Added a native **History & Saved Content** Settings screen with an explicit reading-history consent switch.
- The nonsecret consent preference uses a private, versioned Android SharedPreferences value. Missing, malformed, or future values default to **disabled**, and a wrong-type legacy value is treated as disabled rather than crashing startup.
- The Compose state survives configuration recreation and is persisted across normal app restarts. The screen makes clear that no verified article/history integration is currently connected; enabling consent does **not** collect history or authorize networking.
- Pure Kotlin JUnit tests cover opt-in, opt-out, missing values, future encodings and malformed input.

## Qualification boundary

This is **not** a working reading-history feature. The consent preference is not yet connected to the Rust `UserState` opt-in/history-generation API, cross-process privacy invalidation, stored visit deletion, real-article rendering or physical Android testing. Android SharedPreferences `apply()` alone does not prove durable synchronous revocation. Do not mark MOB-004 or STATE-001 complete until those boundaries are implemented and verified.
