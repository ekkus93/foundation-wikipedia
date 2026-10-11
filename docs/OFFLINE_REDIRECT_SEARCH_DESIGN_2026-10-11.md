# Offline redirect search implementation notes

The current STORE-004 FTS5 catalog rebuild indexes canonical article title, display title, aliases, and structured text. It skips PageRecord::Redirect, so searches for redirect-only titles cannot find their canonical article.

A safe extension should resolve every redirect chain through SnapshotCatalog::resolve_title and append verified redirect titles to the canonical article's indexed text. Use an all-or-nothing transaction; dangling, cyclic, corrupt or over-budget redirect data must roll back the derived index. Results should continue to carry the canonical article key and exact revision, and navigation must revalidate the hit against the catalog.

Regression coverage should include one-hop and multi-hop redirects, Unicode titles, duplicate results, late corruption, missing targets, and prior-index preservation. This is a design note, not completion of STORE-004 or proof of airplane-mode acceptance.
