# Android app shell

This is an **unfinished** native Kotlin/Jetpack Compose application embedding
Android WebView for article HTML. No Wikipedia data is fetched during bootstrap.

Prerequisites: Android SDK Platform 35, build-tools 35.x, JDK 17, Gradle 8.11.1.
From `android/`:

```sh
gradle :app:assembleDebug :app:testDebugUnitTest
```

The shared Rust Android library and UniFFI Kotlin generated bindings are not
present yet. MOB-001 must implement a reproducible Rust `aarch64-linux-android`
build, NDK linking, UniFFI generation, JNI/native packaging and lifecycle-safe
coroutine adapters before the Android app calls the Rust core.

No third-party content is allowed to call privileged native APIs directly. The
bootstrap WebView has JavaScript disabled; future source content needs explicit
sanitization and tightly scoped URI/selection bridges.
