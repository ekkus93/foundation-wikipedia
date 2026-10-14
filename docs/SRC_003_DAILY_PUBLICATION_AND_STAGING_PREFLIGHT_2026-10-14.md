# SRC-003 — Daily checksum origin and staging-tree preflight (2026-10-14)

Status: **partial implementation, not SRC-003 completion**. No source publication, renderer fidelity, or pack acceptance is certified by these changes.

## Implemented

1. `crates/wiki-source/src/public_dump_origin.rs`: the exact official Wikimedia current-content SHA256SUMS URL preflight now accepts any valid Gregorian publication date instead of only the first day of a month. Invalid dates, foreign project names, HTTP, unexpected query strings, and alternate hosts remain rejected. The origin string match is **not** HTTPS certificate validation, a signature, or independent publication authentication.
2. `crates/wiki-source/src/public_dump_strict.rs`: exact staged-export verification inspects the directory tree for unexpected files and symlinks **before** reading checksum members or publication sidecars. A symlinked staging root is rejected; the preflight is not a substitute for keeping staging private against concurrent filesystem mutation.

## Exact-head qualification

- Daily-date implementation: `2782309bebff54b1f037fe5d5de10feab38df18b`. Rust workspace CI 38566304697 failed solely on `cargo fmt --check`; corrected in `0992a2e07eada1f6ae0ae840fd384c468856bd83`.
- Corrected daily-date head: `0992a2e07eada1f6ae0ae840fd384c468856bd83`. Rust workspace CI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38566435162 (success). Platform shells CI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38566435070 (success).
- Strict staging preflight head: `b018e3a230da4bdfe4600b391cde81b62fc86d7f`. Rust workspace CI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38567043334 (success). Platform shells CI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38567043196 (success).
- Tests include valid daily/leap-day dates, invalid Gregorian dates, unexpected staged symlinks, symlinked root, and preservation of the verified exact-tree path.
- Device/OS: GitHub Actions Linux Rust runner and platform CI only; no physical Android or clean desktop acceptance was performed.

## Remaining required for SRC-003

Authenticated publication identity, independent full-source completeness, real public-dump ingestion and revision-matched canonical HTML rendering through a MediaWiki/Parsoid-compatible path, templates/Lua/math/references fidelity, mandatory offline media, and desktop/Android pack acceptance. The canonical TODO entry **must remain unchecked**. The TODO is too large for the currently available full-file source-write path; this supplemental evidence document does not replace its authoritative status.
