# MOB-004 — Persist Android offline choice (2026-10-29)

## Scope

The Android Compose reader previously retained its offline toggle only through a saved
instance state. A full process restart reverted to the online-first default, so Settings
did not yet persist even this non-secret preference.

- Store the explicit offline choice in a private Android SharedPreferences file using
  a versioned string encoding. Initial launch without a saved value remains online-first.
- Preserve `rememberSaveable` for the current Settings screen across configuration
  recreation, without restoring an expanded floating action menu.
- Fail closed to offline for an unknown, corrupt, or future-version string, and for
  an incompatible legacy preference value, rather than crashing or unexpectedly
  enabling network-capable behavior.
- Cover codec roundtrip and malformed/future values in pure Kotlin JUnit tests.

## Validation and limitations

Run `cd android && ./gradlew :app:testDebugUnitTest :app:assembleDebug :app:lintDebug`
(or the equivalent exact-head Platform shells CI gate). This is a source-level
implementation; CI evidence must be attached to the exact commit separately.

The bootstrap WebView continues to block all network resource loads regardless
of this setting. No verified Wikipedia pack, network-capable article transport,
device-backed preference test, or offline media acceptance is claimed. The
preference is intentionally not an authorization to make network requests.
**MOB-004 and MOB-002 remain unchecked.**
