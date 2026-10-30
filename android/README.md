# Android app shell

This is an **unfinished** native Kotlin/Jetpack Compose application embedding
Android WebView for article HTML. No Wikipedia data is fetched during bootstrap.

Prerequisites: Android SDK Platform 35, build-tools 35.x, JDK 17, Gradle 8.11.1.
From `android/`:

```sh
gradle :app:assembleDebug :app:testDebugUnitTest
```

## Rust arm64-v8a ABI build (integration stub)

The `wiki-ffi` crate now produces both an `rlib` and an Android-loadable
`cdylib`. Install the Android NDK, Rust's `aarch64-linux-android` target
for the pinned root Rust toolchain, and export `ANDROID_NDK_HOME`.

```sh
rustup target add aarch64-linux-android
export ANDROID_NDK_HOME="$HOME/Android/Sdk/ndk/<installed-version>"
cd android
gradle :app:buildRustArm64
# Equivalent: bash scripts/build-rust-android.sh
```

The opt-in task uses NDK clang for API 26 and copies the output to
`app/src/main/jniLibs/arm64-v8a/libwiki_ffi.so`. That directory is ignored
by Git. The task deliberately does not run on ordinary `assembleDebug` yet.

A dedicated [Android Rust ABI qualification workflow](../.github/workflows/android-native-abi.yml)
cross-compiles `wiki-ffi` for arm64 with Android NDK 27.2.12479018
and API 26, verifies the ELF target, and retains the `.so` as a CI artifact.
The workflow is path-filtered to native dependencies and can be triggered
manually. **This is an ABI-target build check, not a callable FFI contract.**
The workspace's strict `unsafe_code = "forbid"` lint currently prevents
exporting unmangled C symbols without a separately reviewed safety boundary.
Normal debug APK assembly does not build Rust unless explicitly requested.

### UniFFI code-generation integration stub

A versioned bootstrap UDL now lives at
`crates/wiki-ffi/src/foundation_wikipedia.udl`. It intentionally exports no
callable functions yet. To exercise the Android build integration with an
explicit, pinned `uniffi-bindgen` executable:

```sh
export UNIFFI_BINDGEN=/path/to/pinned/uniffi-bindgen
gradle :app:generateUniFfiKotlin
```

The Gradle task calls `android/scripts/generate-uniffi-kotlin.sh` and writes to
`app/build/generated/source/uniffi`. The generator is caller-supplied during
BOOT-002 so this scaffold does not silently add or update Rust dependencies.
Automated tests exercise the task's UDL/output arguments with a fake generator
and its fail-closed missing-tool path.

**Not a working UniFFI runtime integration:** generated Kotlin is not compiled
into the app, the Rust library has no UniFFI scaffolding/exported API, and
there are no versioned DTOs, coroutine adapters, cancellation bridge or
device-tested Rust calls. MOB-001 owns that production boundary. An APK or
generated Kotlin file alone is not proof of a native bridge.

The Android development shell has native Compose **Settings** categories,
System/Light/Dark appearance, a versioned private **Work offline** preference
and an explicit opt-in **reading-history consent** preference. Settings category
navigation survives configuration recreation; transient FAB actions and Chat
do not. Chat opens a dismissible native bottom sheet but sends no data to an
AI provider. Bookmark remains unavailable until a verified article is loaded.
The placeholder WebView stays mounted when Settings or Chat is displayed.

**Privacy and offline durability:** A user-triggered history-consent change
uses synchronous `SharedPreferences.commit()` before the UI can show consent
enabled. Failed writes leave consent disabled for the current session and show
a warning that a previously saved choice might reappear after restart. Consent
is re-read from storage after activity recreation, rather than restoring a
potentially stale saved Compose value. Offline changes also commit synchronously
before allowing online mode; failed writes keep the current session offline
and display an error. Both settings have pure Kotlin/JUnit failure-path tests.
See [scoped durability report](../docs/MOB_004_ANDROID_DURABLE_PRIVACY_AND_OFFLINE_SETTINGS_2026-10-30.md).

**Important:** These switches are presentation preferences only in this
bootstrap. The WebView always blocks network loads, JavaScript, DOM storage,
file/content access, mixed content and top-level URL navigation, even when
offline mode is off. The manifest grants no Internet permission. The only
rendered content is bundled bootstrap HTML, not Wikipedia content. No reading
history is collected and there is no Rust `UserState` consent bridge.
These settings do **not** prove persistent revocation across a failed disk
write, installed packs, qualified media, production article navigation or
device-tested lifecycle/accessibility behavior. Native Rust bindings, verified
article/media rendering, bookmark/history integration, cross-process privacy
coordination and physical-device acceptance remain outstanding.

See [scoped Android shell qualification](../docs/MOB_002_004_ANDROID_OFFLINE_SETTINGS_SHELL_2026-10-28.md).
