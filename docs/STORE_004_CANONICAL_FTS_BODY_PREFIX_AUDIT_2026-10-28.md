# STORE-004 — Canonical FTS body-prefix audit (2026-10-28)

## Problem

A derived SQLite FTS5 index could retain correct project, stable page ID, exact revision and title while its searchable body was truncated or replaced. The previous catalog membership audit would accept the row, even though its text no longer represented the digest-verified canonical article.

## Implementation

- Refactored canonical searchable-text construction into `canonical_search_text(&Article)`, shared by indexing and verification in `crates/wiki-store/src/search_index.rs`.
- During `verify_catalog_membership`, stream the stored FTS `body` alongside the article identity, re-read the canonical digest-verified frame, and require the body to begin with the exact text assembled from lead blocks, nested sections, display title and aliases.
- Reject bodies exceeding the index byte limit or with malformed non-space-prefixed suffixes. Redirect titles appended by the existing two-pass builder are still permitted as a suffix; this deliberately does **not** claim that every redirect title is authenticated.
- Preserve bounded per-article comparison and the existing 100,000-article audit cap; no entire-corpus body collection is introduced.

## Regression and exact-head qualification

The existing multi-edition fixture now updates an FTS body to `forged text` while preserving project, page ID, revision and title. The audit fails closed with `SearchError::StaleIndex`, and a rebuild from immutable catalog frames restores the index. The other missing, duplicate, stale-revision, forged-title and corrupted-shard cases remain covered.

Implementation: `334cb15d91166cf65400016d4ba29e4efa96b835`.

- [Rust workspace — 40755725616](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40755725616): passed (format, strict Clippy and workspace tests).
- [Platform shells — 40755725764](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40755725764): passed.
- [Android Rust ABI — 40755725449](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40755725449): passed.
- [Android record codec probe — 40755725387](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40755725387): running at observation.

## Remaining qualification

This is **not** an FTS token-posting integrity proof. Corruption of the inverted index without changing the stored body can still escape detection. The redirect suffix is not compared to the exact catalog redirect inventory. Full source authenticity, required media completeness, installed-pack activation, desktop/Android reader wiring and real-device airplane-mode acceptance are also outstanding. **STORE-004 stays unchecked.**
