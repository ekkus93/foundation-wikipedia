# SRC-003 — Native observed census from pinned bzip2 shards

**Date:** 2026-10-23
**Status:** Qualified implementation slice; SRC-003 remains unchecked.
**Exact code SHA:** 9687bc795ab0204f1f60071e23e88ca0f3b3e36d

## Implementation

PinnedObservedCensus::decode_native_bzip2_member reads the next declared member descriptor from PinnedInventory, not a caller-supplied checksum. It composes the portable MultiBzDecoder, bounded compressed and decoded bytes, exact compressed SHA-256 and EOF, XML text/envelope validation, bounded semantic page/revision extraction, page-range and revision-date checks, and the existing namespace-specific observed census writer. Each member must succeed before the pipeline advances to the next shard.

A decoding, checksum, semantic metadata or downstream census failure permanently invalidates the pipeline. Any observed census bytes already written to the private staging sink are provisional and cannot be returned by finish after failure.

## Qualification

- Exact code SHA: 9687bc795ab0204f1f60071e23e88ca0f3b3e36d.
- Rust workspace CI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/39981692344 — passed format, strict Clippy and tests.
- Platform shells CI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/39981692597 — passed.
- Synthetic two-shard test uses actual bzip2-compressed MediaWiki XML, verifies namespace-zero observed IDs 1 and 201 in order, excludes nested contributor IDs, and checks exact decoded byte counts.
- Negative test pins an incorrect member SHA-256 while supplying valid bzip2/XML and verifies that digest failure prevents finish from returning partial census bytes.

## Limits

The census is derived from the same decoded dump and is NOT an independent expected census. A separately authenticated publisher source, independently verified expected page IDs with compatible generation/namespace semantics, full XML compatibility, MediaWiki/Parsoid rendered article fidelity, mandatory offline media/attribution, pack activation and desktop/Android device qualification are still required. Do not label SRC-003 complete or publish a source snapshot based on this slice.
