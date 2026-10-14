# SRC-003 — Public current-content XML and JSONL ordering audit (2026-10-14)

**Status:** Partial implementation; SRC-003 remains unchecked in the canonical TODO.

## Implementation

The production Python `extract_current_dump` path now enforces strictly increasing outer page IDs within each independently checksum-verified, declared shard partition. This replaces its redundant in-memory page-ID set with one previous-ID counter, while retaining revision-ID collision detection.

The multi-shard `extract_current_export` path independently validates decoded JSONL records before database insertion and final publication: strictly increasing page IDs within the exact member partition, positive signed-64-bit revision IDs, exact project identity and checksum-member SHA-256 identity. This is a second boundary against an inconsistent or replaced decoder. Failure prevents final artifact publication.

Regression fixtures include two valid pages reordered in the source XML and decoded-JSONL forgeries of project, member digest, out-of-partition page ID, boolean page/revision IDs and an overflowing revision ID. Each is rejected without publishing the output.

## Exact-head qualification

- Code commit: `cdc9f85f6582b2d54fb239bb0194789731f1b8a3`.
- Rust workspace CI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38550993337 — success at exact code SHA; includes `python3 -m unittest discover -s scripts`, fixture provenance, Rust format/Clippy/tests.
- Platform shells CI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38550993187 — success at same exact code SHA.
- Device: CI-host synthetic fixtures only; no real Android or installed desktop acceptance.

## Remaining

A digest-matching caller-supplied inventory does not authenticate Wikimedia publication or establish independently complete source coverage. Raw wikitext is not rendered by Parsoid/MediaWiki, and required images/math/CSS, attribution, signed pack trust, offline installation and real-device acceptance remain unqualified. SRC-003 must stay unchecked until all acceptance criteria are evidenced.

**Canonical TODO note:** This evidence file supplements `docs/WIKIPEDIA_AI_READER_TODO.md`; the large TODO could not be atomically upserted through the current bounded source-write interface during this run. No checklist item has been marked completed.
