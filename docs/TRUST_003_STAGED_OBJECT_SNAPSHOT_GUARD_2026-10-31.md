# TRUST-003 — Staged object metadata-snapshot hardening (2026-10-31)

## Scope

The shared `wiki-pack-format` staged-object verifier now compares object metadata before opening, after opening, after reading, and against the final pathname. On Unix this includes device/inode, single-link count, size, and modification/change timestamps with nanosecond fields. On other targets it compares file kind, size, and available modification/creation timestamps. A same-size rewrite or pathname replacement detected during verification fails closed with `UnsafePath`.

## Regression coverage

The Unix regression test verifies that a stable object passes the snapshot check, a same-length rewrite is detected, and a replacement inode containing identical bytes is detected. Existing staged-tree digest, size, hardlink, symlink, and undeclared-entry tests remain applicable.

## Boundaries

This is defense in depth. Metadata sampling cannot prove race freedom against a hostile concurrent writer, especially on filesystems with coarse timestamps. Callers must continue to hold an exclusively owned, private, immutable staging tree. This does not authenticate a publisher, validate mandatory offline media, or activate/install a pack. TRUST-003 and PACK-004 remain open pending their complete acceptance criteria.

## CI

Initial implementation `5d2327d0f2c21414f4d7c60cbb4db4a153085bd2` failed Rust formatting at the test assertion. Corrected implementation `f3fe58a60096bc381acafa34b8603de7c90e2cd5` passed all three exact-head workflows:

- [Rust workspace — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41221261133)
- [Platform shells — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41221261043)
- [Android Rust ABI qualification — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41221261259)

The acceptance claim is limited to this staged-object metadata guard and synthetic regression tests; it does not close TRUST-003 or PACK-004.
