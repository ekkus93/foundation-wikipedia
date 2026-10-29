# STORE-001 / RAG-002 — Redirect-target-pinned offline citation navigation (2026-10-28)

## Implementation

The shared Rust `OfflineReader::open_exact_page_target_revision` accepts a saved source `ArticleKey` (including redirect source page IDs), expected resolved target page ID and exact revision. It follows the existing digest-verified bounded redirect chain, then checks both resolved target identity and revision. A redirect retarget cannot silently satisfy an old citation when the new target happens to share a revision number. Invalid expected target IDs fail before reading the catalog; a genuinely uninstalled source returns `None`. The method additionally rejects a cross-project resolved target. The title-based saved-navigation method now shares the same target-ID/revision comparison helper.

## Synthetic regression

The existing on-disk SQLite catalog and immutable-shard fixture exercises a redirect to page 42 at revision 1042, correct target opening, wrong target ID with matching revision, stale revision, zero/overflow expected IDs, missing source page, and a direct article source. Existing digest corruption and stale-title checks remain in the same suite.

## Qualification boundaries

This is a local-only evidence-navigation contract, **not** a proof that the snapshot is publisher-authenticated, that mandatory offline visuals/attribution are installed, or that desktop/Android citation UI is wired. The catalog/shard directory must remain immutable during use. **STORE-001 and RAG-002 remain unchecked.**

Exact-head CI results for the source commit must be recorded before treating this slice as qualified.
