# SRC-003 — Expected census structural preflight

**Date:** 2026-10-24  
**Status:** Scoped software improvement verified; SRC-003 remains incomplete.

## Change

A SHA-256-matching expected census could previously be malformed, unsorted,
duplicated or missing a terminal LF; the native public-dump audit would only
discover this after decoding every potentially huge bzip2/XML shard.

The new `public_dump_census_stream::validate_page_id_stream` reuses the
final auditor's constant-memory cursor: it checks the project/generation/
namespace, strictly increasing nonzero SQLite-safe IDs, nonempty census and
100-million-ID ceiling. `preflight_expected_census` invokes it using the
existing strict `PageIdLines` parser after hashing the expected file, and
checks source metadata again afterward. This happens before any source
member opens. The full post-decode comparison, independent expected digest
recheck and atomic no-overwrite publication remain unchanged.

## Regression and exact-head CI

Five correctly SHA-256-pinned invalid expected-census fixtures test
duplicate IDs, descending IDs, truncated LF, nondigit records and zero
IDs. Each must fail before a source-member open and without publishing a
census. The existing real native two-shard bzip2 roundtrip still runs.

- Initial source: `a68223770f6022c09b68217c13c0533c295ebff0`.
- Corrected LF fixture: `4d4bc14fb180f231aa258e8d9a489b3695fc957e`.
- Qualified code after rustfmt: `090b0d50dc693c65602a56a30a21bc245d88b343`.
- [Rust workspace — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40163226421).
- [Platform shells — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40163225792).

## Scope limit

The caller still supplies both source inventory and expected-census pins.
This is not proof of Wikimedia publisher authenticity, independence of
census provenance, snapshot cutoff, complete upstream coverage,
revision-matched HTML, mandatory offline media, pack installation or
desktop/Android device acceptance. SRC-003 remains unchecked.
