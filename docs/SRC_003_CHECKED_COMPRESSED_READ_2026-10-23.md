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

## Pinned census integration (follow-up)

The Rust `PinnedObservedCensus::decode_checked_member` method now binds
`decode_checked_member` to the existing fail-closed page event and shard
validation state machine. A parser callback consumes the exact compressed
reader and emits semantic `PageEvent` values. A member is closed only if
the decoder returns success *and* fully consumes the stream without I/O
or digest error. Any failure poisons the census pipeline, so callers cannot
replay a later shard or publish a successful census from pages emitted before
the failed digest check.

Synthetic positive and negative tests cover an exact digest-pinned two-shard
census, a wrong digest after valid page events, and a decoder that returns
early with unread trailing bytes.

- Initial code SHA `d9e556e65d6c4b18336307b81cc936a9ac496be3` failed rustfmt in
  [Rust CI 39938805710](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39938805710).
- Corrected exact source SHA **`3e8dc9639c897b07b4fbba1be7b3578067988604`** passed
  [Rust workspace CI 39938942593](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39938942593)
  and [Platform shells CI 39938942735](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39938942735).

The callback still must implement real portable bzip2 and strict streaming
MediaWiki XML decoding before this becomes an operational dump importer.
The digest expected by the pinned inventory must be independently authenticated;
no check in this module proves completeness, publisher authenticity or
offline article/media fidelity. **SRC-003 remains unchecked.**

## Pinned-inventory trust-boundary correction (2026-10-23)

The initial integrated API accepted a caller-supplied `DumpChecksum`.
Even with strict page ordering, that would let a caller give the expected
member filename paired with an independently forged digest, bypassing the
already pinned publication's bytes. The API now selects the next member
descriptor *exclusively* from its own `PinnedInventory`. The method
accepts only the compressed input stream, size budget and semantic decoder.
The same object tracks verified member order, including members consumed
by the existing lower-level event API.

The compressed reader also rejects zero-byte inputs despite a matching
empty SHA-256 and retries up to eight transient interrupted reads. A new
synthetic regression covers the empty-member failure. All previous
multi-member, corruption and partial-decoder tests remain in the workspace
suite. This closes a caller-digest substitution risk in the new composition
seam; it does not validate actual bzip2 framing or XML documents.

- Initial correction commit `7b0e7df1d8c39b916d674b084022899f70446142`:
  [Rust CI 39939761242](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39939761242)
  failed formatting.
- Corrected code SHA **`faa74885767bb8451c8054054c0f4cc32aa1ff81`**:
  [Rust workspace CI 39940054347](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39940054347)
  passed, and [Platform shells CI 39940054398](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39940054398)
  was running when this note was written. Verify its final conclusion independently.

**SRC-003 remains unchecked** until the real source, completeness, renderer,
mandatory offline media and device acceptance gates pass.
