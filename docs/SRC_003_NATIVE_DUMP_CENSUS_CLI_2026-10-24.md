# SRC-003 — Native public-dump census CLI

**Date:** 2026-10-24  
**Status:** Scoped diagnostic implemented and tested; SRC-003 is incomplete.

## What changed

The `wiki-pack` CLI now exposes the existing portable Rust native bzip2/XML
member decoder and digest-pinned independent census gate as a single operator
command. This avoids requiring a custom Rust program or manual semantic event
injection to audit staged, real compressed public-dump shards.

`audit-public-dump` requires exactly eleven positional arguments:

1. Private staged source directory containing `SHA256SUMS` and its member paths.
2. Wikimedia project (for example, `enwiki`).
3. Pinned export generation date (`YYYY-MM-DD`).
4. SHA-256 of the **raw** `SHA256SUMS` bytes, obtained independently.
5. Path to separately obtained expected namespace page-ID census.
6. SHA-256 of the raw expected census file.
7. Nonnegative namespace integer (usually `0` for articles).
8. Private output directory, distinct from source.
9. New output filename (no existing file may be overwritten).
10. Per-member compressed-byte budget.
11. Per-member decoded-byte budget.

Example with placeholders that **must be replaced** by independently verified
publication metadata and appropriately sized limits:

```sh
cargo run --locked -p wiki-pack -- audit-public-dump \
  /private/staged/enwiki enwiki 2026-10-01 \
  '<SHA256SUMS_SHA256>' /private/independent/enwiki-mainspace.ids \
  '<EXPECTED_CENSUS_SHA256>' 0 /private/audit-output \
  observed.ids 1073741824 17179869184
```

The staged source and output roots must already exist, must not overlap, must
contain no symlinked ancestors and must be private (0700 on Unix). The
expected census must be outside both roots and must pass the existing
independent digest/identity/symlink checks. Source members must be bounded,
nonempty regular files, with no symlinked ancestors or multiple hard links
on Unix. The command does not fetch or authenticate upstream bytes.

The tool validates the inventory project/generation and checksum, decodes
**every** inventory member with the native multistream bzip2/XML page
metadata pipeline, enforces compressed and decoded byte budgets and
member digests, audits the observed namespace IDs against the independently
pinned expected file, and publishes the observed census only after the
whole operation passes. Failure leaves no new final census. The output is a
diagnostic receipt, not an installable Wikipedia pack.

## Automated qualification

Implementation: `3b9c6d9d56da92367fc255840d17aa0e45208f81`. Follow-up
formatting, dependency/lockfile and expected-census symlink-path repairs:
`64e6382a83406aff8ad8a6b7ff8b131f3618c426`,
`107c2e48969c29b9c543bcf893270983a868cbdf`,
`acf0659291db6d2dea35eefb79aaf5c182b8ab56`, and
**`f42260d4fbfd65e6b9789b9545929d6341723041`**.

On the final code SHA:

- [Rust workspace 40151408765 — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40151408765): `cargo fmt --all -- --check`, strict Clippy, and workspace tests including the native two-shard CLI fixture.
- [Platform shells 40151408906 — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40151408906): desktop and Android shell checks.

The new test covers a real bzip2-compressed two-shard XML export with
project-scoped siteinfo and namespace IDs, exact positive census matching,
atomic refusal to overwrite an existing output, invalid output filename,
wrong expected digest, mismatched page IDs, corrupt compressed shard, and
Unix symlink rejection for both the expected census and a source member.

## Explicit limitations and next work

The CLI accepts SHA-256 pins from the caller; it **cannot prove** their
authenticity, independent publication origin or compatible snapshot cutoff.
The native XML adapter is a bounded page-metadata subset, not full revision
wikitext-to-Parsoid rendering. The observed census is not an independent
expected census, and successful local matching alone cannot establish
publisher-backed completeness. Full source provenance, generation/census
authentication, revision-matched HTML, mandatory offline media, snapshot
activation and physical-device acceptance are still open.

Do not check SRC-003 complete based on this diagnostic.
