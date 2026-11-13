# ADR 0006 — SQLite FTS5 for offline pack search

**Status:** Accepted (owner decision, 2026-11-12)  
**Date:** 2026-11-12  
**Requirements:** STORE-004, RAG-001, PACK-001, MOB-002, SPEC §16

## Context

The spec asks for BM25/structural lexical retrieval, "potentially Tantivy for local pack search", and no mandatory vector database (§16). STORE-004's open subtask is "Benchmark Tantivy or equivalent; implement current-article retrieval block index and pack search."

Pack search is already implemented on SQLite FTS5 in `wiki-store::search_index`:

- **Index shape.** One `offline_article_fts` virtual table with an exact canonical DDL (`project`, `page_id`, `revision_id` UNINDEXED; `title`, `body`; `tokenize='unicode61'`). Ranking is `bm25(…, 8.0, 1.0)`, weighting title over body, with deterministic tie-breaking by page ID. Edition (project) filtering happens before ranking and `LIMIT`.
- **Derived and rebuildable.** `replace_persisted_from_catalog` and `replace_persisted_from_verified_shards` rebuild the index from the immutable catalog and shards into a private staged file. It is audited for complete catalog membership and FTS5 native `integrity-check`, then published by an atomic rename with directory syncs. Content, media and user state are never touched (STORE-003 evidence `1900b48c`, `15eae27b`).
- **Verified opening.** `open_verified_read_only` and `VerifiedOfflineCatalogReader` refuse an index whose schema, membership or catalog binding does not match. Search results are re-read from digest-verified shard frames, never from FTS text.
- **Bundled engine.** SQLite comes bundled through `rusqlite` 0.32 (`libsqlite3-sys` 0.30.1, `bundled`). That build already ships in the desktop and Android ABI CI. The catalog and user state use the same engine.

Current-article retrieval for chat (RAG-001) is a different problem. It covers one revision of one article and lives in `wiki-search::ArticleLexicalIndex`, built in memory from the open article's blocks.

## Decision

1. **Keep SQLite FTS5 as the v0.1 offline pack search engine.** Do not add Tantivy or another search engine dependency.
2. **Current-article block retrieval stays in memory** (`wiki-search`), built per opened revision. It has no persisted index, because one article is small and its evidence IDs are revision-bound.
3. **The FTS5 index stays a derived artifact.** It may be shipped in a pack as `search.sqlite` or rebuilt by the installer from verified shards (`pack_install`, `bd3b8104`). It is never canonical content.

## Alternatives considered

- **Tantivy:** BM25 with block-max WAND, so common-term top-k queries would likely be much faster than FTS5's full posting scan. It was rejected for v0.1:
  - It adds a large dependency tree to the Android and desktop builds.
  - Its index is a multi-file segment directory needing its own integrity, atomic publication and recovery design.
  - It duplicates verification guarantees that FTS5 already has: canonical schema identity, membership audit, native integrity check, and atomic single-file publication.
  - It adds a second storage engine beside the SQLite catalog and user state.
  - **Not benchmarked here.** This ADR records the owner's decision to standardize on SQLite, not a measured head-to-head.
- **In-memory corpus index (`wiki-search::CorpusLexicalIndex`):** fine for fixtures and evaluation. It would cost a rebuild on every launch and RAM proportional to the pack, which is unsuitable on mobile.
- **Embeddings or hybrid retrieval:** deferred to RAG-004 as an optional derived index (§16). It is not required for pack search.

## Measured evidence

Probe: `crates/wiki-store/tests/fts_search_probe.rs` (ignored by default). It runs the production path: canonical shard frames, SQLite catalog, `replace_persisted_from_catalog` with its audits, `open_verified_read_only`, then raw `OfflineSearchIndex::search` and `OfflineReader::search_articles`.

```
WIKI_FTS_PROBE_ARTICLES=<n> cargo test -p wiki-store --release --test fts_search_probe -- --ignored --nocapture
```

**Corpus:** deterministic synthetic English-like text. Each article has a title, 2 lead and 6 section paragraphs, about 480 words drawn roughly Zipf-shaped from a 20,000-term vocabulary. **Machine:** AMD Ryzen 7 5825U, 27 GiB RAM, Debian 13, Linux 6.12, Rust 1.85.1, release build. Each query shape was sampled 150 times with `limit = 20`.

| Articles | Shard | Catalog | FTS index | Catalog build | Index build (audited) | Verified open |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 5,000 | 9.1 MB | 1.3 MB | 38.1 MB | 4.4 s | 1.7 s | 0.67 s |
| 20,000 | 36.5 MB | 5.3 MB | 150.7 MB | 17.5 s | 7.2 s | 2.7 s |
| 50,000 | 91.4 MB | 13.5 MB | 376.4 MB | 43.7 s | 19.1 s | 6.6 s |

Query latency (p50 / p95, milliseconds). "Raw" is FTS5 ranking only. "Reader" adds decoding and verifying each hit's shard frame.

| Articles | Common term, raw | Common term, reader | Two mid terms, raw | Two mid terms, reader | Rare term, raw | Rare term, reader |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 5,000 | 2.9 / 3.2 | 3.8 / 4.2 | 3.3 / 3.9 | 4.2 / 4.8 | 0.3 / 1.0 | 1.2 / 1.9 |
| 20,000 | 12.9 / 13.3 | 14.0 / 14.5 | 14.2 / 16.7 | 15.2 / 17.6 | 1.2 / 4.5 | 1.9 / 5.4 |
| 50,000 | 33.2 / 33.9 | 34.2 / 34.9 | 36.1 / 42.0 | 37.1 / 43.4 | 3.4 / 11.5 | 4.3 / 12.4 |

What the numbers show:

- Index build, verified opening and size all scale linearly with pack size.
- Shard verification of 20 hits adds about 1 ms.
- Rare-term queries stay in single-digit milliseconds at 50,000 articles.
- Queries containing a term present in nearly every article scale linearly, at about 0.66 µs per article. FTS5 scores the whole posting list before `ORDER BY bm25 … LIMIT`. This is the main cost of not adopting Tantivy.
- The index is about 7.5 KB per article, roughly 4× the zstd-compressed shard. The canonical schema stores `title` and `body` as FTS content, so article text is held in both the shard and the index.

**Caveat:** random synthetic vocabulary is not real Wikipedia text. Real text has different compression ratios, term distributions and stop-word frequencies. These are desktop numbers; Android will be slower.

## Consequences

- **No new engine.** There is no new search dependency, on-disk format or recovery protocol. Pack search shares SQLite with the catalog and user state, and keeps its existing verification and rebuild guarantees.
- **Cap stays in force.** The existing `MAX_ARTICLES = 100_000` per-index cap stays. v0.1 offline content is built as topic packs, and the spec's explicitly deferred list includes a whole-Wikipedia vector database.
- **Known costs, not yet acted on:**
  - Common-term latency grows with pack size.
  - The index stores article text a second time.
  - Mitigations that keep FTS5 include a contentless or external-content table, stop-word handling, prefix or `detail=` options, or limiting the candidate set before ranking.
  - Each of these changes the canonical DDL, the schema-identity check and the rebuild audits, so each needs its own STORE-004 change and evidence.
- **Revisit when:**
  - real-device qualification (MOB-002) misses the search latency budget the owner sets for target packs;
  - real packs need more than 100,000 articles;
  - or measured index size on real pack text makes installs impractical.

  At that point, benchmark Tantivy head-to-head on real pack text before changing engines.

## Validation

- The probe above is committed and reproducible. Existing STORE-004 qualification documents cover schema identity, membership, native integrity and verified opening; the STORE-003 documents cover the rebuild path.
- STORE-004 stays unchecked. This ADR closes its "Benchmark Tantivy or equivalent" subtask as a decision with measured FTS5 evidence. Airplane-mode search across an installed topic pack (its acceptance) still needs real packs and device UI.
