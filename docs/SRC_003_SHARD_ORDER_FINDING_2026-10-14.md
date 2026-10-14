# SRC-003: Numeric ordering of pinned public-dump shards

**Date:** 2026-10-14  
**Status:** Partial SRC-003 implementation; canonical TODO remains unchecked.

Legacy MediaWiki current-content dump shards can be numbered
`current1`, `current2`, ... `current10`. The SHA256SUMS parser sorts
filenames lexically, which places `current10` before `current2`. This
is not the correct sequence for validating increasing page IDs.

The pinned-inventory constructor now sorts validated members by the
numeric lower page-ID bound *after* verifying the generation contract.
The original checksum bytes and their SHA-256 digest remain unchanged.
The existing shard-correspondence test now uses numeric order. A new
ten-member regression test supplies reverse-order checksums, verifies
the canonical member sequence and successfully feeds all ten members
through `StreamShardAudit`.

**Qualified implementation SHA:** `ece49d77c754ce13d01dcb69ef5acaf5cdf160eb`.

**Exact-head CI:** Rust workspace passed, run
[38561966359](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38561966359).
Platform shells passed, run
[38561966467](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38561966467).

**Limitations:** This is an ordering and integrity guard, not proof of
publisher authenticity, source completeness, real XML/bzip2 decoding,
MediaWiki/Parsoid rendering, offline required media, or desktop/Android
real-device acceptance. Do not mark SRC-003 complete.

**Additional modern-shard regression:** `1a48f01409e060e2754c987d8fab59b1bb38dee6`
also tests out-of-order modern `p1p100`/`p101p200`/`p201p300`/
`p1001p1100` names. Exact-head Rust workspace CI
[38562954921](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38562954921)
and Platform shells CI
[38562955031](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38562955031)
both passed. No source-completeness or real-device qualification is implied.
