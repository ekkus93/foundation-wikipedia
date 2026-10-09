# Foundation Wikipedia

A Wikipedia-first reader with optional grounded AI, offline topic packs, and a shared Rust core.

**Status:** Design/implementation bootstrap. No end-user binaries exist yet.

- [Authoritative product/technical specification](docs/WIKIPEDIA_AI_READER_SPEC.md)
- [Canonical implementation checklist](docs/WIKIPEDIA_AI_READER_TODO.md)

## Rust core

Requires Rust 1.85.1 (see `rust-toolchain.toml`).

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo run -p wiki-pack -- --help
```

Library crates under `crates/` are platform-independent. `wiki-ffi` is reserved for UniFFI bindings; Tauri/React and Kotlin/Compose platform shells are planned under BOOT-002. The CLI is intentionally nonfunctional until PACK-004 and must not suggest packs exist yet.

Development uses `master` as the integration branch; inspect the canonical checklist for status. No AI provider is required to read Wikipedia.
