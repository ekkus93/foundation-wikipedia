# SRC-004 — Verified local title navigation with revision-pinned online HTML caching

**Date:** 2026-10-28 (America/Los_Angeles)  
**Repository:** `ekkus93/foundation-wikipedia`  
**Corrected source SHA:** `53d2f337780c4a2a8176c9c293a17c2d4a97e664`

## Scope

The host-independent `wiki-source::online_fetch` API now provides `fetch_title_with_local_index_and_cache`, combining the existing independently verified `LocalTitleIndex` fallback policy with bounded `OnlineRevisionCache` reuse. The original `fetch_title_with_local_index` API remains uncached, preserving compatibility. Both paths share one implementation of index validation, title/redirect resolution and explicit fallback selection.

The new API does **not** certify a caller's local source index: the caller must already have authenticated the source and verified article and mandatory offline media completeness. Online HTML, including cached HTML, remains a text preview rather than an offline-complete installed article.

## Required behavior and regression coverage

- In explicit offline mode, indexed canonical titles and redirect aliases resolve to verified local article versions without any network activity or online-cache access. Unknown titles remain missing.
- In online mode, title and redirect mapping is freshly queried before current-revision metadata. Cached HTML is returned only for the complete freshly validated revision identity (project, page ID, revision ID, title, namespace and timestamp). Repeated titles require fresh title/metadata responses even on cache hits.
- When title transport is unavailable, an independently indexed local revision remains available only as a labeled noncurrent fallback. Metadata-transport failures preserve the known local fallback, but stale HTML is never presented as current.
- If upstream rebinds an indexed alias to a different page ID, the identity conflict is a selection failure and does not promote unrelated installed content.
- Synthetic tests for these contracts live alongside `online_fetch.rs` and require no Wikimedia credentials, Android device or external HTTP access.

## Exact-head evidence

- Initial API commit `1172028bc66a1cb22843330d4896a4c4db3c8460` failed Rust formatting (Rust run 40653263027); corrected formatting commit `389e5f3bdd179f4f6cf4dd1d9bb79d2a147d5c8f` accidentally omitted the tail of the test module because of an incomplete bounded file read (Rust run 40653817522).
- Full module restored and formatting reconciled at `53d2f337780c4a2a8176c9c293a17c2d4a97e664`. [Rust workspace CI 40654079934](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40654079934) passed (format, Clippy and workspace tests). [Platform shells CI 40654079806](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40654079806) passed.

## Remaining

SRC-004 stays open: a persistent revision-aware cache, verified live Wikimedia interoperability, production Android TLS reader integration, independently authenticated and complete official source data, offline-required visuals and attribution, offline pack/reader integration and real-device acceptance remain to be completed. This is an isolated source-layer improvement rather than a product acceptance result.
