# SRC-004 / DUI-001 — Desktop search input preflight

**Date:** 2026-10-24  
**Status:** Scoped implementation verified; top-level tasks remain open.

## Problem and fix

The desktop search form previously allowed a query up to 512 JavaScript
UTF-16 code units and sent the trimmed text to `search_wikipedia` even when
the shared Rust `safe_navigation_text` policy would reject the query. In
particular a multi-byte Unicode query can exceed the Rust 512-byte UTF-8
limit while fitting the HTML input's code-unit limit. Control characters,
Unicode noncharacters and invisible formatting controls were likewise
allowed to reach the IPC boundary before failing.

`normalizedReaderSearchQuery` now trims user-entered outer spacing and
validates it using the existing exact-title Unicode and UTF-8 predicate.
Both desktop search and exact-title buttons disable invalid input and
show a precise validation hint. The actual search handler checks the
normalized value again before invoking the native command. Valid
multilingual input remains supported; internal text is not rewritten.

## Evidence

The Node regression suite in `desktop/scripts/readerResponse.test.mjs`
covers normal whitespace trimming, CJK/emoji titles, exact UTF-8 byte
boundaries, controls, zero-width/bidirectional formatting, noncharacters
and unpaired surrogate code units. No device or live upstream run is
claimed from these synthetic tests.

- Initial code: `18ce6d8ab34cde73290158ac7b2a070f447f4e06`.
- Test fixture escape correction: `f0e74e988f51c9a4dcec5822826a026ed943c207`.
- Integrated qualifying head: `d41e22fe2ab61f865f84fbbb7aac46212ddbf40e`.
- [Rust workspace — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40147438846).
- [Platform shells — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40147438754), including frontend typecheck and Node tests.
- [Desktop launch smoke — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40147438973).

The earlier Platform/Desktop failure on `f0e74e9` was an independently
introduced escaped-string fixture in the concurrent link-label tests;
the actual Unicode scalar fixture was corrected at the qualifying head.

## Remaining acceptance

This preflight does not prove authoritative live Wikimedia source
interop, revision-matched visual fidelity, mandatory offline media,
safe installed-pack reading, Android parity, or real-device acceptance.
Keep SRC-004 and DUI-001 unchecked until the SPEC's complete
acceptance requirements are satisfied.
