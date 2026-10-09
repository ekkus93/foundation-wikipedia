# Foundation Wikipedia development environment

This document describes the bootstrapped projects. Requirements and completion are governed by [SPEC](WIKIPEDIA_AI_READER_SPEC.md) and [TODO](WIKIPEDIA_AI_READER_TODO.md).

## Rust libraries and CLI

Rust **1.85.1** is pinned in the root `rust-toolchain.toml`. Requires Cargo, rustfmt and Clippy; no Wikimedia account, LLM or downloaded dataset is required for unit tests.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo run -p wiki-pack -- --help
```

The nine library crates are platform independent and **must not** import Tauri or Android APIs. The CLI currently only prints bootstrap help; no packs are built.

## Desktop (Tauri 2 / React / TypeScript / Vite)

Requires Node.js **22**, npm, Rust **1.90.0** for native Tauri (pinned in `desktop/src-tauri/rust-toolchain.toml`), and Linux WebKitGTK 4.1 development dependencies for native builds.

On Ubuntu 24.04:

```sh
sudo apt-get update
sudo apt-get install -y libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf libssl-dev
cd desktop
npm install
npm run build
npm run tauri dev
```

To check native Tauri while at repo root (avoid accidental root Rust 1.85 override):

```sh
cargo +1.90.0 check --manifest-path desktop/src-tauri/Cargo.toml
```

The React shell displays a static placeholder in Wikipedia-style typography and a nonfunctional action hub. Real article fetching and WebView bridge are future tasks. Desktop npm dependencies are directly pinned; a committed transitive lockfile and installer qualification remain TODO work.

## Android (Kotlin / Compose / WebView)

Requires **JDK 17**, **Gradle 8.11.1**, Android SDK Platform 35 and build-tools 35.0.0. The APK uses minimum API 26. Install Gradle and Android SDK separately; the repository does not yet include Gradle wrapper artifacts.

```sh
cd android
gradle :app:assembleDebug :app:testDebugUnitTest --no-daemon
```

Debug APK expected at `android/app/build/outputs/apk/debug/app-debug.apk` after a successful build. This is not proof of real-device installation until tested. Android shell shows a static local article placeholder with JavaScript disabled and file/content access blocked. No Internet permission is requested yet.

### Planned shared Rust integration

MOB-001 remains pending. Target `aarch64-linux-android` via Android NDK and `cargo-ndk` or equivalent reproducible cross compilation; produce an Android-safe Rust shared library in the ABI-specific `jniLibs` location, generate UniFFI Kotlin bindings from versioned Rust DTO/use cases, and expose coroutine-safe async calls with lifecycle cancellation. Android must not load raw desktop Tauri APIs. Validate the bridge on a real Android device and on CI.

## CI and evidence

`.github/workflows/rust.yml` checks root Rust fmt, Clippy, unit tests and exact Git SHA. `.github/workflows/platform-shells.yml` builds the desktop frontend, Android debug APK/tests and checks Tauri native Rust dependencies on Ubuntu 24.04. A clean CI build does not mean the prototype shell has been installed or exercised on a device.

Do not mark canonical TODO tasks complete until their tests and platform acceptance have been recorded at the exact commit SHA.
