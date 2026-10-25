# SRC-003 — Native census fail-fast preflight

**Date:** 2026-10-24  
**Status:** Scoped source audit robustness qualified; SRC-003 remains incomplete.

## Change

The `wiki-pack audit-public-dump` operator command previously started
decompressing every verified dump member and only discovered an unavailable
later member when it reached that shard. It now preflights the complete
pinned `SHA256SUMS` member inventory, rejecting missing, symlinked,
nonregular, empty, oversized or multiply-hard-linked inputs before any
bzip2/XML decoding begins. Every member is reopened and checked again
during its actual SHA-256-verified read; preflight alone never confers
publisher trust or substitutes for byte verification.

The shared `stage_and_audit_pinned_census` gate previously discovered
an already existing final census name only after full staging, decoding,
and expected-census comparison. It now checks for occupied destination
names before staging, treating dangling symlinks as occupied as well.
The existing atomic no-overwrite `hard_link` publication is preserved,
so a competing writer appearing after preflight still cannot be
overwritten. This is a performance and operational-safety optimization,
not a new source-completeness guarantee.

## Tests and exact-head evidence

- Shared gate regression asserts zero member-open callbacks when a
  previously published output already exists.
- Unix regression rejects a dangling symlink destination before
  reading any member and cleans up its fixture.
- The CLI two-shard bzip2/XML fixture rejects a missing later shard
  without publishing an output; existing corruption, digest, symlink,
  independent-census and successful-roundtrip cases remain covered.
- Initial implementation `52579abd201a5ffadd863b2b76d652d55dbf1bab`
  failed rustfmt on [Rust run 40154753410](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40154753410).
- Formatting fix `c513179031311a8c65280e6de47762aeb2cc467e`
  exposed an unremoved symlink test fixture on [Rust run 40154816969](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40154816969).
- Qualified source head **`fe955ba2db86d4a9c9b199c6995f4969aa7ec3e9`**
  passed [Rust workspace 40155181140](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40155181140)
  and [Platform shells 40155181129](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40155181129).

## Remaining acceptance

The CLI still requires caller-supplied SHA-256 pins and a separately
obtained expected census. These are not automatically authenticated
as Wikimedia publications, do not prove source-generation completeness
or cutoff equivalence, and do not yield revision-matched rendered HTML,
required offline media, an installable pack or device acceptance.
Keep SRC-003 unchecked until its independent source qualification and
the wider product acceptance requirements are satisfied.
