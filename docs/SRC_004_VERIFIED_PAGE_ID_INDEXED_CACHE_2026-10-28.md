# SRC-004 — Verified page-ID offline lookup and revision-pinned online cache

Date: 2026-10-28. Scope: shared Rust source navigation and synthetic regression coverage, not end-to-end offline pack acceptance.

## Change

`LocalTitleIndex` now indexes independently verified, offline-complete canonical entries by both normalized title and stable Wikimedia page ID. `resolve_page_id(page_id)` returns the same `LocalSearchHit` (project/page ID, title, exact local revision and location) as canonical title resolution. Redirect aliases remain title-only metadata; they cannot rebind a stable page ID. Invalid, incomplete, foreign-project, online-current and duplicate catalog records continue to be rejected during construction. A missing page ID returns no hit.

`wiki-source::online_fetch::fetch_page_id_with_local_index_and_cache` accepts an optional verified index and a bounded `OnlineRevisionCache`. In offline mode, it returns only an indexed local revision or `MissingOffline`, without contacting the transport or reading online-preview HTML. It checks the project/page identity before the offline fast path and refuses a catalog for another project. In online mode, it fetches current revision metadata before reusing sanitized, exact-revision HTML and carries a separately labeled verified local fallback on upstream failures. Cache contents cannot become offline-complete by virtue of having been viewed online.

The existing `fetch_title_with_local_index_and_cache` similarly combines verified local title/redirect fallback with fresh online title resolution and current-revision checks. Desktop Tauri has a separate process-local online preview cache; neither desktop nor Android is yet connected to a complete, authenticated local catalog and media store.

## Regression evidence

- `local_title_index::tests::verified_page_id_lookup_matches_title_identity_and_never_follows_aliases` checks canonical and Arabic titles, stable IDs, missing/zero/overflow IDs, and exact provenance parity with title lookup.
- `online_fetch::tests::indexed_page_id_cache_never_promotes_online_html_to_offline_article` checks no HTTP in offline mode, exact-revision online reuse after fresh metadata, a labeled local fallback on upstream failure, foreign-project catalog rejection, and invalid project/page IDs rejected before offline selection.
- `online_fetch::tests::indexed_title_cache_preserves_verified_local_fallback_and_rechecks_official_identity` checks redirect identity revalidation, cache reuse, no HTTP offline, upstream failures, and alias rebinding rejection.

Source commits: `53d2f337780c4a2a8176c9c293a17c2d4a97e664` (indexed title/cache, Rust workspace and Platform shells passed), `be5aee9e484d2c1d09a00448656fd36f376c26c7` (page-ID lookup), `ff41d858fd802738a5a2c34d8f4c06ca3eb66f25` (formatting), `f6fd4041b8251e4139b963640249875038346cdb` (identity parity). Rust workspace CI run 40655168985 passed for the identity-hardening source commit. Platform shells run 40655169043 was pending when this document was drafted. Final documentation-head CI must be checked independently.

## Remaining acceptance gaps

Publisher-authenticated whole-source generation and independent completeness; revision-matched mandatory offline visuals, captions, maps, diagrams, equations, licenses and attribution; persistent verified catalog/media activation; production desktop/Android offline navigation and rendering; live Wikimedia API interoperability; physical-device acceptance. The new local index accepts only caller-verified versions and is **not itself** a media or source authenticator. Do not mark SRC-004 complete on the basis of these synthetic tests.
