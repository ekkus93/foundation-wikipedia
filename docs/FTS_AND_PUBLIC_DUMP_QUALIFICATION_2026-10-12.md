# Offline search and public-dump source qualification — 2026-10-12

## STORE-004: FTS5 title ranking (partial)

SQLite FTS5 BM25 weights are positional across five columns: three unindexed
identity fields, canonical title and body. The former two-weight invocation
mistakenly weighted the unindexed columns. The corrected search uses
`bm25(offline_article_fts, 0.0, 0.0, 0.0, 8.0, 1.0)`.
The new regression requires the exact canonical title to outrank ten repeated
body mentions. Verified search-result membership is tested without assuming
unrelated terms have a fixed BM25 order.

## SRC-003: checksum inventory and staged receipt access-time checks (partial)

A read may update filesystem access time without changing a source file.
Comparing full `os.stat_result` values caused false inventory and receipt
mutation alarms on CI. Checks now compare stable device/inode/size/mtime/ctime/
link-count identity and byte length; the inventory test simulates access-time
drift. Checks still require caller-controlled private staging and do not
authenticate an upstream Wikimedia publication.

## Exact-head qualification

- Code and regression tests qualified at `fefcfe1f2b49afd2053c5ee3026279c6cb9e2c29`.
- Rust workspace (fmt, Clippy, Rust tests, Python source fixtures):
  https://github.com/ekkus93/foundation-wikipedia/actions/runs/38347967653 — passed.
- Platform shells:
  https://github.com/ekkus93/foundation-wikipedia/actions/runs/38347967628 — passed.
- Earlier failed heads exposed and resolved title weighting, independent
  BM25 ordering assumptions, and access-time false positives.

Both STORE-004 and SRC-003 remain unchecked in the canonical TODO: this
increment does not establish authenticated complete Wikimedia publications,
MediaWiki/Parsoid rendering fidelity, mandatory offline media, full pack
installation or desktop/Android offline acceptance.
