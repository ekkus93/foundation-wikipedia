# Offline FTS5 article search (STORE-004 partial)

**Status:** Verified deterministic fixture index only. Not yet wired to a fully authenticated installed snapshot, pack builder, Android reader, or release acceptance.

## Implemented boundary

`wiki_store::search_index::OfflineSearchIndex` stores a **derived** SQLite FTS5 index with the canonical article's project/page ID, exact revision ID, title and text from validated structured blocks. Section headings, paragraphs, lists, tables, infobox labels/values, mathematical source text and footnotes are indexed. Raw HTML fallbacks and binary media are deliberately excluded rather than being interpreted as trusted plain text.

`rebuild(&[Article])` replaces the index in one SQLite transaction. Invalid articles, duplicate page identities, oversized text and SQLite failures roll back the transaction; an older complete index remains queryable. An empty input deliberately clears the index. A persisted SQLite database can be reopened and rebuilt without downloading source data again. Queries are bounded to 8192 UTF-8 bytes and 32 Unicode alphanumeric tokens, quoted before binding to FTS5 MATCH; ranking uses BM25 with title boost and stable project/page-ID ties, capped at 100 results. No search path initiates network requests.

## Tests

`cargo test -p wiki-store --test offline_search --locked` checks title-vs-body ranking, Unicode matching, query limits/operator treatment, duplicate-identity rollback, exact-revision replacement, persistence and full rebuild. Workspace CI also runs formatting, strict Clippy and Rust tests. These are synthetic canonical article fixtures, not a representative offline topic pack.

## Remaining acceptance

This is **not STORE-004 completion**. Required: feed the index from authenticated immutable snapshot/catalog records without materializing the full corpus in RAM; add pack installation/activation ownership and corruption recovery; benchmark large multilingual article sets and disk/Android RAM; integrate actual desktop/Android offline search and source-version labeling; and qualify airplane-mode behavior. Index contents must never confer publisher trust or offline-ready status. All required visual media and attribution remain a separate pack-completeness gate.
