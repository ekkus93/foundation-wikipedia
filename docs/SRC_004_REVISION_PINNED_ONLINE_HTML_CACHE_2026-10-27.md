# SRC-004 — Opt-in revision-pinned online HTML cache

**Date:** 2026-10-27 (America/Los_Angeles)  
**Repository:** `ekkus93/foundation-wikipedia`  
**Qualified source SHA:** `54c3fa078abbaf75a1b39a97940852e114f6bd7f`

## Scope

Added `crates/wiki-source/src/online_cache.rs`, a bounded in-memory LRU for `OnlineRenderedRevision` objects accepted by the existing exact-revision HTML parser. Defaults are 32 records and 16 MiB of HTML; callers may choose 1–128 records and 1 byte–64 MiB. The cache exposes entry and HTML byte counts, but its internal insertion/read APIs are crate-private so callers cannot insert arbitrary unsanitized HTML.

`online_fetch.rs` now exposes `fetch_article_with_cache`, `fetch_page_id_with_cache`, and `fetch_title_with_cache`. Existing APIs retain uncached semantics.

**Freshness invariant:** Every online lookup still performs a fresh current-revision metadata request. Title navigation additionally performs fresh title/redirect resolution. The cached HTML is reused only when the entire `OnlineRevision` (project, page ID, revision ID, title, namespace, timestamp) matches the fresh metadata, not merely when the title or page ID matches. On a miss, the exact oldid HTML is fetched, verified and sanitized before cache insertion. A failed render cannot populate the cache.

**Offline invariant:** An offline lookup never reads this cache and never invokes HTTP. Sanitized online HTML alone cannot prove complete images, diagrams, math or attribution. It is not an installed pack, an offline-ready article or authenticated publisher evidence. Upstream metadata/transport failures remain explicit failures with only independently verified local fallback versions.

## Regression evidence

- Unit tests cover bounded LRU eviction, invalid limits, oversized entry rejection, full revision identity, title/timestamp drift, changed revision IDs, fresh metadata required on repeat page visits, title redirect re-resolution, shared cache use across page-ID/title navigation, no HTTP or cache promotion offline, and no cache poisoning on rejected rendered HTML.
- Source integration first passed exact-head [Rust workspace 40644552955](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40644552955) and [Platform shells 40644552944](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40644552944) at `4569c8d51d9e0d654198866dce3241459c499862`.
- Final title/page integration at `54c3fa078abbaf75a1b39a97940852e114f6bd7f` passed [Rust workspace 40644886681](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40644886681) and [Platform shells 40644886591](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40644886591). Intermediate formatting-only failures were corrected before qualification.

## Remaining

This cache is opt-in and process-local. Production desktop and Android reader wiring, persistent revision-aware cache storage, real live Wikimedia interoperability, independently authenticated source completeness, mandatory offline media/attribution and end-to-end offline/device acceptance remain open. SRC-004 stays unchecked.
