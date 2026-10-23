# SRC-003 — Bind native dump member decoder to the pinned inventory

Date: 2026-10-23
Status: Scoped source-integrity improvement; SRC-003 remains unchecked.

## Threat and fix

`ingest_pinned_bzip2_member` accepted a caller-provided `DumpChecksum`
and passed its digest into the compressed-input verifier. The
`PinnedPageIngest` previously validated only the member path and
page-ID shard range against the pinned inventory. A caller could
therefore substitute a matching filename and an attacker-controlled
SHA-256 to authorize different compressed bytes through this otherwise
"pinned" decoder.

The `StreamShardAudit` now exposes an internal comparison of the
entire expected next `DumpChecksum` against the caller descriptor.
`PinnedPageIngest` rejects matches in an active or failed state.
`ingest_pinned_bzip2_member` requires that exact descriptor match
**before starting the shard or decoding any input**; mismatch aborts
the ingest and returns `PinnedDecodeError::UnpinnedMember`.

## Regression evidence

- Forged digest with the correct shard filename is rejected.
- Correct digest with a different inventory member path is rejected.
- In both cases `finish()` fails after the attempted substitution.
- Existing positive compressed native bzip2 ingestion remains valid.
- Initial SHA `a3330929ad1eb369f4ebbf0cdc169dc338b4334c`:
  [Rust CI 40012407168](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40012407168)
  failed a test-expression rustfmt-only check.
- Corrected exact source SHA `28ee5b940f143e63b8e052eef49842ed8a11c352`:
  [Rust workspace 40012611356](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40012611356)
  and [Platform shells 40012611155](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40012611155)
  passed.

## Limits

The descriptor comparison binds to the **existing** pinned inventory,
not independently authenticated Wikimedia publication. This does not
establish independent expected page census, publisher authenticity,
generation completeness, MediaWiki/Parsoid rendering, required offline
media, or physical desktop/Android acceptance.

**SRC-003 remains unchecked.**
