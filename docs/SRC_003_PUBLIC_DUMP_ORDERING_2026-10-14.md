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

## Follow-up: source slot declarations and decoded payload fidelity guards

- The XML shard extractor now rejects inconsistent MediaWiki slot model/MIME pairs, including wikitext declared with a non-wikitext MIME type and non-wikitext declared as `text/x-wiki`. The check covers main and additional slots. Source commit `e74210e2ef1f07d959f1ec442543fe9f44e61562` passed [Rust workspace CI 38557462185](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38557462185) and [Platform shells CI 38557462100](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38557462100).
- The combined-export assembler now independently revalidates decoded JSONL namespace/title, Gregorian UTC revision timestamp, main-slot model/MIME and wikitext/source-text consistency, redirect metadata, revision ancestry, and bounded/unique additional slot metadata before publication. A synthetic forged decoder cannot launder these malformed fields into the combined export. The first code/test heads `3f30538a88cb4e439d800f2a54a886a4749d8c7f` and `488f21781070b8b6f0b8d3324553daa247e383e0` failed fixture tests because the forged-JSONL test wrote escaped rather than actual line endings and then used a literal escaped newline in a title. Those test errors were corrected without disabling the guards.
- **Qualified exact code/test SHA:** `dea7b613dc49e09c1b7b925ef3b18742416eb81e`, with [Rust workspace CI 38558436462](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38558436462) and [Platform shells CI 38558436497](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38558436497), both successful. Rust CI includes Python synthetic regression tests and fixture provenance verification.
- **Limitations:** Shape and internal consistency are not a byte-for-byte proof that an independently supplied decoder reproduced the upstream XML. No official authenticated complete snapshot, independently qualified page census, Parsoid/template/Lua/math/reference rendering, mandatory media/attribution, signed pack, installed offline reader, or physical-device acceptance is established. **SRC-003 remains unchecked.**

## Follow-up: bounded per-shard revision uniqueness and metadata fan-out

- `scripts/extract_public_dump_pages.py` now uses a private, disk-backed SQLite unique index for per-shard revision IDs instead of an in-memory Python set that grows with every page. The index and any temporary JSONL are cleaned after successful publication and after rejected duplicate revisions. This bounds duplicate-detection RAM independently of shard size; the overall XML decoder still has a separate bounded decompressed-byte budget.
- Source SHA `5ced4abaded744e83145a53397b90789e539985a` passed [Rust workspace CI 38558820876](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38558820876) and [Platform shells CI 38558821050](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38558821050), including the new successful and duplicate-revision cleanup tests.
- The same extractor now rejects whitespace-padded/control-bearing page titles and redirect targets, and more than 128 additional revision content slots, before any output is published. The combined JSONL assembler already validates these title/redirect contracts and caps slot fan-out at the independent trust boundary. Source SHA `60388cb4e5cae15e731cdfbef3a5957d9ef07a4d` passed [Rust workspace CI 38559326099](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38559326099) and [Platform shells CI 38559326097](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38559326097).
- These are synthetic ingestion/resource-safety improvements, **not** a qualification of independently authenticated Wikimedia dumps, complete source coverage, Parsoid rendering, mandatory offline media or physical device installation. **SRC-003 remains unchecked.**
