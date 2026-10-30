# MOB-004 — Durable Android history consent and offline preference (partial)

**Date:** 2026-10-30 (America/Los_Angeles)  
**Scope:** Native Android bootstrap Settings state; not an authenticated article reader, Rust privacy bridge or physical-device qualification.

## Implementation

1. **Reading-history consent:** Removed asynchronous `LaunchedEffect` / `SharedPreferences.apply()` for opt-in and revocation. User-triggered changes now use `commit()` and are reflected as enabled only after a successful synchronous write. A failed/throwing write disables consent in the current UI session and displays an explicit warning that an earlier saved choice may reappear after restart. Consent is re-read from private storage on activity recreation rather than restored from a stale `rememberSaveable` value.
2. **Work offline:** Removed asynchronous preference writes. Both FAB and Settings toggle now use a shared synchronous update. A failed/throwing write forces the current session offline, preventing an unpersisted request to go online from being presented as successful. The user sees a snackbar and, in Settings, an inline warning.
3. **Tests:** Pure Kotlin JUnit tests exercise successful enable/disable and online/offline commits, false-return failures and thrown storage errors. Existing malformed/future/wrong-type preference decoding and Settings navigation regressions remain in place. The Android shell compiles and tests through Platform shells CI.

## Exact source commits and CI

- History consent: `40d78ba3ff91de76cc4ce901caede30be582e602`; [Rust workspace passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41028718149), [Platform shells passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41028718163).
- Offline preference: `5e9622cb0da8851be20e79a319d6138b7f658bae`; [Rust workspace passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41028989703), [Platform shells passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41028989571).
- Separate Android emulator-install workflows [history](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41028718152) and [offline](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41028989636) were still running when this report was prepared. Recheck before claiming those results.

## Qualification limits

This does **not** integrate the Rust `UserState` privacy generation, implement a durable cross-process revocation protocol, prove atomicity across Android and Rust stores, collect or delete visits, connect a real network transport, or render authenticated Wikipedia articles. If a storage write fails, the app cannot guarantee the previously durable setting is changed; warnings explicitly disclose this. The WebView still blocks network loads regardless of the offline setting. No physical Android device, actual process kill during a write, low-storage failure or TalkBack interaction was exercised. **MOB-004 and STATE-001 remain unchecked.**
