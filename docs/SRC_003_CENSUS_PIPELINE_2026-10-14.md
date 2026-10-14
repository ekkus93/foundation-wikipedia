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
