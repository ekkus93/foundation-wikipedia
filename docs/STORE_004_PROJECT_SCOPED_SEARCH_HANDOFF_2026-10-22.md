# STORE-004 project-scoped offline lexical search — implementation handoff (2026-10-22)

Current baseline: `6d0f10a3c0b6d55c4cd6518de019f97069c4328a` on `master`. Ralph Bridge reads and exact-head CI are functional; the current head passed Rust workspace and Platform shells.

## Gap

`OfflineSearchIndex::search(query, limit)` searches all installed Wikipedia editions. A caller showing a single edition cannot request project-scoped FTS ranking before the result limit, so filtering the returned list after retrieval could silently omit valid results from that edition. `OfflineReader::search_articles` inherits the same limitation.

## Implementation

1. Add `OfflineSearchIndex::search_project(project, query, limit)`. Validate `ArticleKey { project, page_id: 1 }` before query execution. Refactor `search` and `search_project` through a common internal query with `WHERE offline_article_fts MATCH ?1 AND (?2 IS NULL OR project=?2)`, retaining the existing BM25 ordering and maximum 100 results.
2. Add `OfflineSearchIndex::search_verified_project(catalog, root, project, query, limit)`, resolving every hit against the immutable frame and exact revision via `resolve_hit`.
3. Expose `OfflineReader::search_articles_project` through the same verified path.
4. Add in-memory FTS tests with identical query terms in `enwiki` and `frwiki`, asserting independent result limits, no cross-edition leakage, valid empty results, invalid project rejection, and backward compatibility. Add an offline-reader integration test using a verified on-disk catalog.
5. Run Rust fmt, Clippy, workspace tests, and exact-head Platform/Android CI. Keep STORE-004 unchecked until full real offline-pack search and device qualification.

## Current blocker

Two attempted Ralph Bridge source-write calls were rejected by OpenAI safety checks before execution. No files were changed, and the existing `master` head remains intact. This handoff is documentation only and does not qualify STORE-004.
