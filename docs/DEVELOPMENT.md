# Development environment (bootstrap)

The authoritative requirements are in [WIKIPEDIA_AI_READER_SPEC.md](WIKIPEDIA_AI_READER_SPEC.md), and completion states are in [WIKIPEDIA_AI_READER_TODO.md](WIKIPEDIA_AI_READER_TODO.md).

## Supported bootstrap environment

- Rust 1.85.1, with Cargo, rustfmt and Clippy installed by `rustup`
- Linux x86_64 for initial Rust CI; Android and desktop shells are future BOOT-002 work
- No Wikimedia account, LLM or dataset is needed to run core bootstrap tests

```sh
rustup show
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo run -p wiki-pack -- --help
```

Do not treat scaffolding as a working reader or pack builder. Platform-specific build prerequisites, Cargo/Gradle/npm lockfile policy and installation instructions will be expanded with BOOT-002.

### Dependency boundaries

The shared Rust crates must not depend on Tauri, React, Compose or Android classes. Adapters wrap the Rust services. Tests must pin source revisions and not require network services.
