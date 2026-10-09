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
now cross-compiles `wiki-ffi` for arm64 using Android NDK 27.2.12479018
and API 26, verifies the ELF architecture and exported C symbol
`foundation_wikipedia_ffi_abi_version` (returns `1`), and preserves the
native `.so` as a short-lived CI artifact. The workflow is path-filtered to
native dependencies and can be triggered manually. This checks the native
build/ABI artifact, **not** Kotlin JNI interoperability or a working UniFFI
generated interface. Normal debug APK assembly still omits the shared Rust
library unless the opt-in build is invoked.

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
