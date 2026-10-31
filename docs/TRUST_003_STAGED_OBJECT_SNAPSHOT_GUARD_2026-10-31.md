# TRUST-003 — Staged object metadata-snapshot hardening (2026-10-31)

## Scope

The shared `wiki-pack-format` staged-object verifier now compares object metadata before opening, after opening, after reading, and against the final pathname. On Unix this includes device/inode, single-link count, size, and modification/change timestamps with nanosecond fields. On other targets it compares file kind, size, and available modification/creation timestamps. A same-size rewrite or pathname replacement detected during verification fails closed with `UnsafePath`.

## Regression coverage

The Unix regression test verifies that a stable object passes the snapshot check, a same-length rewrite is detected, and a replacement inode containing identical bytes is detected. Existing staged-tree digest, size, hardlink, symlink, and undeclared-entry tests remain applicable.

## Boundaries

This is defense in depth. Metadata sampling cannot prove race freedom against a hostile concurrent writer, especially on filesystems with coarse timestamps. Callers must continue to hold an exclusively owned, private, immutable staging tree. This does not authenticate a publisher, validate mandatory offline media, or activate/install a pack. TRUST-003 and PACK-004 remain open pending their complete acceptance criteria.

## CI

Exact-head Rust workspace and platform shell CI must be checked after the implementation commit. No CI result is presumed by this document.
