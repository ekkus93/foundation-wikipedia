# STORE-004 — Verified published FTS open and staged audit (2026-10-28)

## Problem

An existing SQLite FTS5 database can have a valid schema and return digest-verified search hits while omitting other installed articles. The preceding catalog-membership audit could detect this, but opening an existing index and publishing a rebuilt one did not invoke it automatically.

## Implementation

- Added `OfflineSearchIndex::open_verified_read_only(path, catalog, root)`. It opens the published index with the existing read-only/schema/path guards and then streams the complete immutable catalog and FTS article identities through `verify_catalog_membership`. A missing/extra/stale article or corrupt catalog frame fails before a verified reader is returned. The caller must keep the source snapshot immutable and independently authenticate it.
- `replace_persisted_from_catalog` now audits the newly rebuilt staged FTS database before fsync and atomic replacement. A verification failure leaves the previous published index untouched.
- Added a disk-backed regression with a single revision-scoped `enwiki` article: verified opening and search work; deleting its derived FTS row rejects verified opening; a derived-only rebuild restores the reader; modifying canonical shard bytes then fails digest verification.

## Exact-head verification

- Initial implementation `c8b95edc59831c7e9cb82dccc9016fd811a38d2d` failed rustfmt (extra blank line), [Rust workspace 40748399242](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40748399242).
- Corrected source `1ce3778cb4e01565cfebe9ecb1d2c374931c32cc` passed [Rust workspace 40748645910](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40748645910) (format, strict Clippy, workspace tests), [Platform shells 40748646115](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40748646115), and [Android Rust ABI 40748646082](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40748646082). Android record codec probe 40748655503 was in progress at documentation.

## Shared offline reader constructor

The shared Rust `OfflineReader::new_verified(catalog, index, root)` now audits complete canonical article/FTS identity membership before returning a reader. The existing lower-level `new` constructor remains available, so application adapters must explicitly select the verified gate rather than assume every reader is authenticated. The existing disk-backed reader fixture checks successful verified construction/search, refusal of a schema-valid but empty FTS index, and failure when an immutable shard is corrupted.

- Exact source `4ec850d87d87325f4cbe487d41642540daf5e14b`: [Rust workspace 40749497833](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40749497833) passed, [Android Rust ABI 40749497863](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40749497863) passed. [Platform shells 40749497803](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40749497803) and [Android codec probe 40749497792](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40749497792) were running at documentation.

## Remaining qualification

The membership audit checks project, stable page ID, exact revision, title and canonical record digests, not every FTS body byte or token posting. It does not prove Wikimedia publisher authenticity, full source completeness, mandatory offline media, or production pack activation. Both the verified opener and the audited reader constructor are opt-in; neither is yet connected to desktop or Android installed-pack initialization. It requires a caller-owned immutable snapshot and a privately controlled derived-index path. Physical-device airplane-mode acceptance remains outstanding. **STORE-004 remains unchecked.**
