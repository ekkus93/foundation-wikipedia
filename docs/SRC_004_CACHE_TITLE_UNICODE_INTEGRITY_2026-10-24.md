# SRC-004 / SEC-001 — cached online title Unicode integrity (2026-10-24)

## Scope

The temporary, revision-keyed HTML cache previously rejected control characters and selected bidi overrides but accepted invisible formatting characters and Unicode line/paragraph separators in its displayed title. The cache now rejects U+200B, U+2028–U+2029, U+2060, U+206A–U+206F, and U+FEFF, in addition to the existing directional-format guard. Validation applies both before insertion and when reading persisted cache entries.

Regression coverage exercises each rejected marker on write, the newly rejected markers on read even if stored SQLite data is modified, and legitimate Persian ZWNJ and Indic ZWJ titles. The latter remain accepted because those joiners are linguistically meaningful.

## Exact-head qualification

- First source SHA `e1bb0d2c36061b5b19f47b9c0f4a18944e5b3aa8`: [Rust workspace 40125878344](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40125878344) failed `cargo fmt --all -- --check` (test-layout formatting only).
- Corrected source SHA **`293e187cdc9aee12e97f4874f6ffa9d970a4b7b9`**: [Rust workspace 40125937040](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40125937040) passed format, strict Clippy and workspace tests; [Platform shells 40125937016](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40125937016) passed; [Android Rust ABI qualification 40125937022](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40125937022) passed. [Android record codec probe 40125937020](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40125937020) was still in progress at this observation and is not evidence of cached-title behavior.

## Boundaries

This is a scoped display-identity integrity improvement for `SRC-004` and `SEC-001`; it does not authenticate Wikimedia responses, sanitize HTML, validate all upstream title-normalization rules, guarantee cache freshness, or qualify complete offline packs. Both parent tasks remain unchecked. CI evidence is for synthetic hosted tests, not desktop/Android physical acceptance.
