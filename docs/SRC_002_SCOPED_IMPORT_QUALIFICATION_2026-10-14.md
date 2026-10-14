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
