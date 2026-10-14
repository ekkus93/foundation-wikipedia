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

## Follow-up: bounded decoded JSONL and count reconciliation

- Code SHA: `15f1e299fd51e43edaf939572815d80cd1e37bf1`.
- `extract_current_export` now reads each decoded JSONL line with a 64 MiB hard cap (including JSON escaping overhead), rejects a partial/un-terminated line, rejects a non-object record, and reconciles the decoder-reported page count with the number of records actually consumed before publishing.
- Regression fixtures simulate an inflated decoder count, a missing JSONL line terminator and an over-budget decoded line; all fail without a published output.
- Exact-head CI: [Rust workspace 38551634771](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38551634771) and [Platform shells 38551634783](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38551634783), both successful on the code SHA. These tests exercise synthetic boundaries, not independently authenticated real Wikimedia dumps or a Parsoid renderer.
