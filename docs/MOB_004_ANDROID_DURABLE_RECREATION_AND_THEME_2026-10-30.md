# MOB-004 — Android durable preference restoration and appearance writes (partial)

**Date:** 2026-10-30 (America/Los_Angeles)  
**Scope:** Native Compose development shell; not an authenticated Wikipedia reader or integrated Rust user-state store.

## Problem

The Compose shell saved the offline value in the activity's `rememberSaveable` state. On activity recreation, a stale online value could override a newer persisted offline preference. The theme UI also restored a saveable selection and ran an asynchronous `SharedPreferences.apply()` effect, potentially rewriting durable state with an older selection at startup. The UI had no failure indication for a theme write.

## Implementation

- The shell saver now saves **Settings visibility and category only**. Its restore path always obtains the offline value from private, versioned SharedPreferences decoded for the new activity. Missing settings retain the online-first default; malformed/future settings remain offline.
- Appearance state initializes from the durable versioned theme preference on each activity creation, rather than from stale saveable state. No write occurs just because the screen is composed.
- A user-requested theme selection now calls a checked synchronous `commit()`. A successful write activates the selected theme. A false result or runtime storage exception retains the previous active theme and displays an inline failure message. The UI makes no claim that a failed write persisted.
- Pure Kotlin JUnit regressions exercise successful theme persistence, all requested theme modes on failed writes, thrown storage errors, and restoration of a Settings subpage with an updated durable offline choice.

## Verification

- Implementation SHA: `6a07f52c6d5685dc7f68eea6622b6e0abfe345b0`.
- [Rust workspace CI 41035843556](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41035843556): passed.
- [Platform shells CI 41035843555](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41035843555): running when documented; includes Android compilation, unit tests and lint.
- [Android emulator install smoke 41035843494](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41035843494): running when documented.
- Exact-head CI for this documentation commit must be checked independently.

## Remaining acceptance

This does not connect Compose to the Rust `UserState` privacy generation, prove cross-process coordination, implement real bookmarks/history, render authenticated Wikimedia content, install verified media or qualify a physical Android device. SharedPreferences synchronous writes still occur on the UI thread in this development shell; production performance and lifecycle behavior need device testing. **MOB-004 remains unchecked.**
