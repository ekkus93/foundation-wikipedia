# SRC-002 scoped Structured Contents import qualification — 2026-10-14

## Exact-head evidence

- Qualified source head: `2e7b2aed69bc8a90c55dd77439f1478756f2a508`
- Rust workspace: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38572531887 (success)
- Platform shells: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38572531910 (success)
- Both workflows ran against the same exact commit.

## Implementation

`crates/wiki-source/src/enterprise_verified.rs` exposes
`import_verified_enterprise_ndjson_scoped` and
`import_canonical_enterprise_ndjson_scoped`. The canonical path now reuses
the verified scoped path rather than maintaining a second evidence-check loop.
Regression tests exercise a revision-matched live article and reject deletion
markers from a different source generation. Underlying
`enterprise_ndjson.rs` tests cover project mismatch, live/deleted conflicts,
duplicate tombstones, invalid generations, revision mismatch and missing
companion fields.

## Qualification boundary and remaining work

These are bounded, all-or-nothing validation functions for *caller-supplied*
Structured Contents and regular companion NDJSON. They do not authenticate
Wikimedia Enterprise feed origin, prove publication completeness, acquire
mandatory media, or demonstrate real-world snapshot installation.
The canonical `docs/WIKIPEDIA_AI_READER_TODO.md` SRC-002 checkbox therefore
remains **unchecked**. SRC-003 rendering and offline media qualification
remain independent acceptance requirements.

## First-letter link qualification (2026-10-14)

Source head: `0e0787e0e9060139a8090741ee6ec75652428728`.
Rust CI run `38581833366`: success. Platform CI run `38581833124`: success.

Snapshot link indexing and lookup now share first-letter title normalization, preserving all other characters' case. Regression tests cover lowercase-initial links, aliases, fragment retention, and colliding page identities. This does not establish publication authenticity, source completeness, mandatory media or device acceptance; SRC-002 remains unchecked.

## Shared model normalization follow-up

The model alias validator now distinguishes noninitial case and rejects duplicate first-letter-normalized aliases. Qualified model head `0a5855f5487bdb7dc0a85ebc589bf2638492da4a`: Rust CI 38582924419, Platform CI 38582924318, Android ABI CI 38582924191 (all success). The source link resolver delegates to the model's shared title-key function as of `3e99ba436d3cf4246ddec36cb214229d1da76a7b`: Rust CI 38583240914 and Platform CI 38583241117 (both success). Source authenticity and SRC-002 acceptance remain outstanding.

## Digest-pinned scoped Structured Contents import (2026-10-14)

Portable `wiki-source::enterprise_pinned` provides
`import_pinned_enterprise_ndjson_scoped` and
`import_pinned_canonical_enterprise_ndjson_scoped`. These compose the
existing all-or-nothing project/generation/revision-scoped Enterprise
component join and canonical normalization with a streaming SHA-256 check of
**the exact structured and companion NDJSON bytes consumed by the import**.
A matched pin for only one component is insufficient. Malformed pins, valid
but modified payloads, source identity/revision mismatches, and failed
normalization never return a partially accepted canonical batch.

- First implementation SHA `d965958a9331fdb394e8dafe349340ca25f1e358`
  failed [Rust workspace CI 38685143930](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38685143930)
  at the formatting check and is not qualified.
- Corrected implementation SHA **`4db23d42e6483abd1194ef9ceea61aa698d7ab01`**
  passed [Rust workspace CI 38685284234](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38685284234)
  (format, Clippy, tests and repository checks) and
  [Platform shells CI 38685284239](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38685284239)
  at that exact SHA.
- Synthetic regression fixtures exercise accepted revision-matched scoped
  components, both independently mismatched SHA-256 pins, malformed digests,
  parseable altered bytes and revision drift despite matching checksums.

**Qualification boundary:** the SHA-256 expectations are caller-supplied
integrity pins. They are **not** independently authenticated Wikimedia
publication receipts or a proof of complete source generation, deletion-feed
coverage, required media or real offline snapshot acceptance. The library
does not download authenticated Enterprise snapshots. No local tests imply
publisher trust. **SRC-002 remains unchecked**.

## Digest-pinned local deletion evidence (2026-10-14)

The portable `wiki-source::enterprise_pinned_deletions` module adds
`decode_pinned_deletions` and `import_pinned_enterprise_with_deletions`.
They accept a **local canonical, versioned** deletion NDJSON stream (not
Wikimedia's raw deletion-feed format), with explicit `schema_version=1`,
`project`, `generation_id`, and `page_id`. Every record must be bounded,
well-formed JSON terminated by LF, with no duplicate fields, schema drift,
truncation, oversized lines or mismatched scope. The entire stream must match
its own caller-provided SHA-256 pin before any live pages can be returned.
The importer passes those validated tombstones into the existing
revision-scoped Structured Contents and regular companion import, where
duplicates and live/deleted conflicts are rejected. An empty deletion stream
may have a valid empty digest but does **not** prove deletion completeness.

- Source SHA **`4f84b138ec3d8d4474e3b3988be9c1dbbffa3127`** passed
  [Rust workspace 38688058871](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38688058871)
  (format, strict Clippy and tests) and
  [Platform shells 38688058814](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38688058814)
  at this exact head.
- Synthetic tests cover matching scoped live/deletion components, empty
  deletion feed, mismatched digest, changed generation, duplicate deletion,
  live-page deletion conflict, missing LF, CRLF, unknown/duplicate JSON fields,
  wrong schema, invalid ID and oversized record.
- During implementation a bounded `get_file` window was mistakenly written
  as the full `crates/wiki-source/src/lib.rs`; CI identified the truncation.
  The **complete original blob** was restored via Ralph Bridge from commit
  `9b5b08f26d2e3cb5f018b52f9af6958bd55c72f8` at commit
  `25914d116dbd09be6c435f2fd72c931ed1d3035c`. The module declaration
  was then reapplied only after a complete three-window read at
  `f8878863c08feb43387f89a5d241c715e22df9f8`.
  No incomplete source file remains in the qualified head.

**Not yet qualified:** official deletion-feed schema adaptation and upstream
publisher authentication, independently verified generation completeness,
mandatory article media, full end-to-end import, or device acceptance.
**SRC-002 remains unchecked.**
