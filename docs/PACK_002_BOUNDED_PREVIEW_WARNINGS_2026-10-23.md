# PACK-002 — Bounded deterministic topic preview warnings

**Date:** 2026-10-23  
**Status:** Scoped partial implementation; PACK-002 remains unchecked.

## Change

The `wiki-pack-builder::selection` resolver now stores at most 256 distinct
preview warnings, retaining the lexicographically smallest messages regardless
of upstream article membership order. If warnings exceed this bound, the result
ends with `Additional topic warnings omitted (limit: 256)`. This avoids
allocating one retained string per redirected/disambiguation page or category
in an otherwise valid large graph. The notice does not claim an exact omitted
count. The warning cap is diagnostic only and does not affect selected page
IDs, canonical definition hashes or snapshot-scoped selection receipts.

## Tests and exact-head CI

A 320-redirect synthetic category graph exercises the bounded warning list,
sorted retained messages, explicit truncation, stable selected IDs and equal
pinned receipts when membership order is reversed. A two-warning fixture
confirms ordinary small previews retain all warnings without truncation.

- Initial code `73c0be8eed9503c4900cf61ee056af5cb4663d91` failed formatting:
  [Rust 40050906584](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40050906584).
- Formatting follow-up `25a49d10acea148882729fd07e23c5bffc68cb66`
  accidentally omitted the source file's final test/module closing lines:
  [Rust 40050947484](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40050947484).
- Restored complete implementation at exact SHA
  `515b94756fd4d5dac4c90ec1c6491bd6cb463dc0`:
  [Rust workspace 40051006522](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40051006522)
  and [Platform shells 40051006526](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40051006526)
  both passed.

## Not yet qualified

The category graph is still caller-supplied and not proven to be a complete,
publisher-authenticated Wikimedia generation. Pack builder UI preview,
mandatory media coverage, full pack build, offline installation and real-device
acceptance remain outstanding. This change must not mark PACK-002 complete.
