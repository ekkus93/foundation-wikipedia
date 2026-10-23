# SRC-003 — Checked compressed-member consumption (partial)

Date: 2026-10-23
Status: Qualified synthetic Rust component; **SRC-003 remains unchecked**.

## Change

Added `crates/wiki-source/src/public_dump_checked_read.rs` with
`decode_checked_member`. It wraps an externally supplied compressed input
stream in a SHA-256 hashing, byte-budgeted `Read` adapter and passes that
adapter to a decoder supplied by the caller. The function returns a
successful value only if the decoder reached actual EOF without encountering
or suppressing an I/O/budget error and the consumed byte digest equals the
independently pinned inventory member digest. It never trusts a prior
pathname-only verification as a substitute for verifying bytes consumed.

Regression cases cover:
- Complete, bounded consumption of an intact member.
- Hash mismatch on changed compressed data.
- Successful-but-premature decoder return while trailing bytes remain.
- Over-budget compressed input even when the decoder discards the read error.
- An I/O failure after a valid byte prefix even when the decoder ignores it.
- Invalid configured limits and a decoder-reported parse error.

## Qualification

The first implementation `a0a6796b21d54b629da5b8650da72f58b1bbc0b8`
failed formatting in [Rust CI 39938184288](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39938184288);
formatting was corrected directly on master.

Qualified exact source SHA:
`564b5286574c336a65f1695e45f9ff085e6a983d`

- [Rust workspace CI 39938276761](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39938276761): passed fmt, strict Clippy, tests, codec probe, and existing source checks.
- [Platform shells CI 39938277026](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39938277026): passed.

## Remaining integration and limits

This is **a reusable read boundary, not an XML or bzip2 importer**.
It cannot validate bzip2 integrity, well-formed XML, page/revision semantics,
multi-member closure, or source authenticity on its own. A future importer
must connect a portable bzip2 decoder and strict streaming XML parser to the
existing pinned `PinnedPageIngest`/census pipeline and use a transactional
staging protocol. A callback can have side effects; callers MUST defer
publishing any normalized records or census until every member and all
independent completeness checks pass. The expected SHA-256 itself MUST come
from separately authenticated Wikimedia publication metadata; integrity
pins are not signatures.

Other release blockers remain: independently authenticated upstream dump
generation and expected census, revision-matched MediaWiki/Parsoid rendering,
templates/Lua/math, mandatory offline visuals with attribution, complete
pack/export/install, and desktop/Android acceptance. Do not mark SRC-003
complete until its explicit acceptance criteria are satisfied.
