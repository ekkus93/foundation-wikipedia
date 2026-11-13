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

`RUST_ANDROID_ABIS` selects ABIs (default `arm64-v8a`; add `x86_64` for the
emulator, which needs `rustup target add x86_64-linux-android`). The
[Android Rust ABI qualification workflow](../.github/workflows/android-native-abi.yml)
cross-compiles `wiki-ffi` for arm64 with Android NDK 27.2.12479018 and API 26
and verifies the ELF target. Normal debug APK assembly does not build Rust.

### Shared Rust bridge (MOB-001)

`wiki-ffi` exports a Kotlin-callable API through UniFFI 0.31 proc macros:
`OfflineLibrary` installs a custom `.wpack`, activates a snapshot, and searches
and opens articles through the shared verified offline reader (no network).
Per the owner decision of 2026-11-13, `wiki-ffi` alone relaxes the workspace
`unsafe_code = "forbid"` to `"deny"` so UniFFI's generated scaffolding compiles;
hand-written `unsafe` is rejected by `scripts/test_ffi_unsafe_boundary.py`.

The generated bindings are committed at
`app/src/main/java/uniffi/wiki_ffi/wiki_ffi.kt`, so the app compiles without a
Rust toolchain. Regenerate them after changing the Rust exports:

```sh
bash android/scripts/generate-uniffi-kotlin.sh   # or: gradle :app:generateUniFfiKotlin
```

This builds the host `wiki-ffi` library and runs the workspace-pinned
`tools/uniffi-bindgen` in library mode with formatting disabled, so output is
deterministic; CI fails if the committed bindings drift. App code uses the
coroutine wrapper `org.foundation.wikipedia.rust.RustOfflineLibrary`, which runs
every call on `Dispatchers.IO`, refuses main-thread calls, and surfaces typed
`LibraryException`s. The bindings load `libwiki_ffi.so` through JNA, so a call
fails at runtime unless the library was built into `jniLibs`.

The [Android FFI bridge workflow](../.github/workflows/android-ffi-bridge.yml)
builds the x86_64 library, checks binding drift, and runs the instrumented test
`RustOfflineLibraryTest` on an API 35 emulator. It installs the committed
fixture pack `fixtures/synthetic/wpack/android-sample-v2.wpack`, then searches
and opens articles through Rust. A physical-device run is still required for
MOB-001.

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
