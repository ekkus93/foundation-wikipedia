# STORE-004 — Native FTS5 integrity check before index publication (2026-10-28)

## Problem

A derived SQLite FTS5 index can have intact stored article identities, titles and body bytes while its internal shadow tables are inconsistent. The existing complete catalog-membership audit verifies canonical article and redirect bytes but does not inspect FTS5's internal postings/docsize structures. A normal search can still appear to work after such corruption.

## Implementation

In `crates/wiki-store/src/search_index.rs`, `OfflineSearchIndex::verify_native_fts_integrity` invokes SQLite FTS5's built-in `integrity-check` special command against a **writable** derived index. `replace_persisted_from_catalog` runs the native check on its staged file after the complete immutable-catalog membership/body/redirect audit, before syncing and atomically replacing the previous published file. The existing staging cleanup and failure propagation preserve the prior published index if the native audit fails.

This check complements the canonical record audit; it does not authenticate source publication or media completeness.

## Regression coverage

`native_fts_integrity_rejects_corrupt_shadow_data_with_intact_article_text` creates a valid in-memory FTS5 index and first confirms the native integrity check succeeds. It then deliberately corrupts the `offline_article_fts_docsize` shadow row through raw SQLite (bypassing FTS5 updates). Stored body text and an ordinary `MATCH` query still succeed, but the native integrity check returns an SQLite error. Existing staged-publication and verified-open tests continue to exercise successful rebuilds.

## Exact-head CI

- Initial implementation `532c5c3ee0ceb79ec2e011c870cb7cfcf49850ed` failed rustfmt: [Rust workspace 40782493555](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40782493555).
- Corrected source `ac18ff2b416f3a5c431128486d83d3667dd4925b`: [Rust workspace 40782575645](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40782575645) passed, including formatting, Clippy and workspace tests; [Android Rust ABI 40782575642](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40782575642) passed.
- [Platform shells 40782575787](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40782575787) and [Android record codec probe 40782575599](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40782575599) were pending at this qualification; their outcomes must not be presumed.

## Remaining limitations

FTS5's native integrity command requires a writable index. The published read-only opener currently performs schema and full catalog/text/redirect checks, **not** this native shadow audit. Full independent token-posting verification for already-published read-only indexes, publisher-authenticated complete Wikimedia source, mandatory offline media, production desktop/Android activation and physical-device acceptance remain outstanding. **STORE-004 remains unchecked.**
