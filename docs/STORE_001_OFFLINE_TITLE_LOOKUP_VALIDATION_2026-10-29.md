# STORE-001 / SRC-004 — Canonical offline title lookup validation (scoped)

**Date:** 2026-10-29  
**Exact source commit:** `c68cbe3a3087a2581210b50d7e893748ba22d9e9`  
**Scope:** Shared Rust model and immutable offline SQLite catalog. **Not** full STORE-001/SRC-004 acceptance.

## Failure mode and fix

The catalog's private `title_key` previously trimmed arbitrary caller titles before normalizing the first Unicode scalar and underscores. Consequently, a lookup for `" Earth"` could resolve the canonical `"Earth"` instead of rejecting the malformed request. Invisible/bidi controls and over-budget title inputs could also be treated as ordinary missing titles. Saved navigation and revision-scoped citation lookups must not silently reinterpret a caller's title identity.

The canonical `wiki-model::valid_navigation_title` validator is now public and reused by the catalog before title normalization. It enforces the same 4,096 UTF-8-byte limit, no outer whitespace, no controls and no invisible/bidi formatting as canonical article/redirect validation. Valid first-letter/underscore normalization and meaningful Persian/Indic ZWNJ/ZWJ remain intact. The existing title index and immutable record formats are unchanged.

## Regression and qualification

- `cargo test -p wiki-store`: new title-key invalid/valid Unicode cases and disk-backed `SnapshotCatalog::lookup_key` / `resolve_title` fail-closed cases; existing tests retained.
- `cargo fmt --all -- --check`, strict workspace Clippy and Rust workspace tests: [run 40837853607](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40837853607), **success** for the exact source commit.
- [Platform shells 40837853584](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40837853584), **success** for the same source commit.
- [Android Rust ABI qualification 40837853608](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40837853608), **success** for the same source commit.
- [Android record codec probe 40837853570](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40837853570) was **in progress** at documentation; its result must be rechecked independently.

This is a synthetic local input-validation regression, not a real installed-pack or live-source qualification. The source archive used for local review matched the exact pre-change Git blobs; source writes used Ralph Bridge's exact-head compare-and-swap transaction. No new branch or PR was created.

## Remaining work

Publisher-authenticated complete source generations and pack signatures, required offline figures/media and attribution, atomic installed-pack activation, desktop/Android production reader wiring, and physical-device/airplane-mode acceptance remain unverified. **STORE-001 and SRC-004 remain unchecked.**
