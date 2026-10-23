# SRC-003 — Bounded decoded-output boundary (partial)

Date: 2026-10-23
Status: Qualified synthetic Rust component; SRC-003 remains unchecked.

## Implementation

`crates/wiki-source/src/public_dump_checked_read.rs` now exposes
`decode_checked_member_with_bounded_output`. It composes the existing
digest-pinned, compressed-byte-budgeted reader with a provisional `Write`
sink that enforces a separately configured decoded-byte limit. A decoder
must consume compressed input through EOF, independently validate bzip2/XML,
and send *all* decompressed bytes through the bounded writer. The helper
returns the writer, decoded byte count and decoder value only after the
compressed digest matches and both I/O boundaries qualify.

The wrapper fails closed on output budget overflow, zero-progress output,
underlying output errors and failed flush, even if the decoder swallows a
write error and returns success. It does not roll back bytes already written
to the underlying sink; callers MUST use private, uncommitted staging and
discard it on any failure. The caller must independently pin the expected
SHA-256 to a trusted publication. The helper is not itself a decompressor
or XML parser.

## Regression evidence

Synthetic unit tests cover successful bounded output, swallowed decoded
budget overflow, swallowed sink I/O failure, mismatched compressed digest,
decoder early return and invalid decoded-byte limit. Existing tests also
exercise swallowed compressed-read failures and compressed-byte limits.

The initial implementation at `889ca23fd84ccfb0808e8fd7412c009957c16108`
failed rustfmt in Rust run 39941020500; the first formatting repair at
`72f0493105e9fd7805e6c4e73b7ada6b165a9f84` failed the final
blank-line rustfmt check in Rust run 39941235026.

Qualified exact code SHA:
`a69c9e9e47f585345f91ee67541ace60e98e1a99`

- Rust workspace: https://github.com/ekkus93/foundation-wikipedia/actions/runs/39941313211 — success.
- Platform shells: https://github.com/ekkus93/foundation-wikipedia/actions/runs/39941313117 — success.

## Outstanding SRC-003 acceptance

This is a composable budget guard, not an actual portable Rust bzip2/XML
decoder. It does not independently authenticate Wikimedia source
publications, establish an expected page census, implement high-fidelity
MediaWiki/Parsoid rendering, resolve mandatory offline media/attribution,
or qualify desktop/Android end-to-end offline reading. Keep SRC-003
unchecked until those requirements and relevant device tests are evidenced.
