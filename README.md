# Foundation Wikipedia

A Wikipedia-first reader with optional grounded AI, offline topic packs and a shared Rust core.

**Status:** Implementation in progress. The desktop and Android projects are **bootstrap previews**, not functioning Wikipedia readers. Search, article retrieval, pack management and AI remain TODO work.

- [Authoritative product/technical specification](docs/WIKIPEDIA_AI_READER_SPEC.md)
- [Canonical implementation checklist](docs/WIKIPEDIA_AI_READER_TODO.md)
- [Clean build instructions](docs/DEVELOPMENT.md)

## Architecture

- Shared Rust workspace: nine platform-independent crates plus the `wiki-pack` CLI.
- Desktop: Tauri 2 Rust adapter with React/TypeScript/Vite UI.
- Android: Kotlin/Jetpack Compose shell with a restricted WebView showing a static placeholder.
- Later: Rust UniFFI bindings, actual normalized Wikimedia article HTML, pack engine, article-grounded RAG and local/remote AI.

## Quick checks

```sh
# Shared Rust core (root Rust 1.85.1)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked

# Desktop UI (Node 22)
cd desktop
npm install
npm run build
npm run tauri dev

# Android shell (JDK 17, SDK 35, Gradle 8.11.1)
cd ../android
gradle :app:assembleDebug :app:testDebugUnitTest
```

Tauri's native Rust adapter separately targets Rust **1.90.0**. See `docs/DEVELOPMENT.md` for Linux native dependencies.

Development integrates directly on `master` under Ralph Bridge. Only TODO items with committed qualification evidence are complete.
