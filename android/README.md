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

**Not a working UniFFI integration:** no UniFFI-generated Kotlin bindings,
versioned exported DTOs, JNI call path, coroutine adapter or device-tested
loading exists. MOB-001 must implement and test these before any app call
can use the shared Rust core. An APK compiling without this library is not
proof of a native bridge.

The Android development shell's reader FAB exposes four labeled, touchable
action buttons (nearest FAB: Chat, Bookmark, Offline, Settings) and responds
with an explicit placeholder snackbar when selected. Android Back collapses the
expanded menu; opening the menu transfers keyboard focus to Chat, and the main
button has an accessible open/close description. Real-device accessibility
qualification remains pending. None of the actions,
article fetches, offline saves or settings screens is implemented yet;
instrumented interaction and screen-reader qualification remain outstanding.

No third-party content is allowed to call privileged native APIs directly.
The bootstrap WebView has JavaScript disabled; future source content needs
explicit sanitization and tightly scoped URI/selection bridges.
