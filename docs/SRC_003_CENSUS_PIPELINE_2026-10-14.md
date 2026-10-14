# SRC-003 — Verified pinned-ingest census integration (partial)

Date: 2026-10-14  
Status: Qualified partial implementation; SRC-003 remains unchecked.

## Implemented

- `public_dump_census_export::ObservedCensusWriter` streams the page IDs observed in one selected namespace to a caller-controlled staging sink.
- `public_dump_census_ingest::PinnedObservedCensus` now connects the verified-member-order and page/revision metadata validators to that exporter. The census is only returned once every declared member was closed and the ingest and writer completed successfully.
- The bridge poisons itself permanently after malformed page events, missing/out-of-range/duplicate IDs, wrong member order, or an output writer failure. Failed sessions cannot return a successful census artifact. Valid events from other namespaces still participate in source ordering but are not exported for the selected namespace.
- Regression tests exercise two pinned shards, cross-namespace filtering, missing final members, duplicate IDs, writer failures, and terminal invalid states.

## Qualification

- Formatting repair commit: `83f2ae1d49fb86a1536ea4bf5d12975f0327acc5`. [Rust workspace CI 38609403917](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38609403917) and [Platform shells CI 38609403818](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38609403818) passed.
- Integration code at exact SHA `7248bff5f2cab122548376958c2044ab432bde35` passed [Rust workspace CI 38610122643](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38610122643) (format, strict lint and tests) and [Platform shells CI 38610122669](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38610122669).
- Both changes were committed directly on `master` using Ralph Bridge v0.6.1; no per-task branch or PR.

## Not yet demonstrated

The caller must still discover/authenticate the original Wikimedia publication, verify each actual compressed member's digest, securely decode bzip2 and well-formed XML into events, and stage the output privately before atomic publication. This *observed* census is **not an independent expected census** and cannot prove that upstream pages were omitted. Full Parsoid/MediaWiki rendering, mandatory offline visuals with attribution, pack installation, and real desktop/Android acceptance remain outstanding. Do not mark SRC-003 complete.

## Additional fail-closed completion tests

Exact source SHA `ce48afe4a9f04434f1f761f5af30b43c1c6711bf` adds integration regressions for a completely validated multi-member stream with **no pages in the selected namespace**, and for a staging writer that accepts all page bytes but fails final `flush()`. Both must return an error without yielding a publishable output handle. Formatting correction of the first test-only commit `5de04bb6d14ffdce2d2f34f68d52dddeac70e278` preceded qualification.

- [Rust workspace CI 38610955304](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38610955304) — passed, exact source SHA.
- [Platform shells CI 38610955599](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38610955599) — passed, exact source SHA.

These tests qualify fail-closed completion behavior only; staged files still require caller-managed deletion/atomic promotion and independently authenticated source data.


## Private staged observed-census publication

Exact source SHA: `43f17e9fe80edb07ea74ded8792db7c3182b60d6`.

- [Rust workspace CI 38616793402](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38616793402) — success (format, Clippy, unit tests), exact SHA.
- [Platform shells CI 38616793052](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38616793052) — success, exact SHA.

`public_dump_census_ingest::stage_pinned_observed_census` writes to a private, restrictive-permission temporary file, requires all pinned semantic events and a successful final flush, syncs the output, and atomically hard-links it to a no-overwrite destination. Failure removes the temporary file; an existing published census is not replaced. Tests cover successful multi-member output, duplicate destination, incomplete/duplicate source events, invalid filenames, and symlink staging root rejection.

**Limits:** the caller must use a private directory that cannot be concurrently modified by an adversary. The callback is responsible for supplying semantic events decoded from independently checksum-verified compressed XML members. The function does not authenticate Wikimedia, decode bzip2/XML, compare an independently sourced expected census, render HTML or publish a qualified offline pack. SRC-003 stays unchecked.
