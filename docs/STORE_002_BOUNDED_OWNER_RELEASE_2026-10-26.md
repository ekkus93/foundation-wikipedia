# STORE-002 — Bounded transactional owner-wide media release

**Date:** 2026-10-26  
**Scope:** Partial STORE-002; no production byte garbage collection or top-level task closure.

## Problem

`MediaRegistry::remove_all_for_owner` previously read an owner's complete
media digest inventory into an unbounded `Vec` inside a default SQLite
transaction, then deleted its ownership rows. A corrupted or oversized owner
inventory could consume unbounded memory, and the transaction did not
reserve the writer lock before enumerating the rows.

## Implementation

- Retain the existing public API and return type, but route it through an
  internal bounded helper with a maximum of 100,000 owner entries, matching
  the owner inventory and batch-install ceiling.
- Begin an `IMMEDIATE` SQLite transaction before reading any owner claims,
  so a competing SQLite writer cannot append claims between enumeration and
  deletion. A lock conflict fails the operation without releasing claims.
- Query at most `limit + 1` digests in stable order. Fail closed on overflow,
  invalid digests, invalid owner identifiers or invalid limits; dropping the
  transaction rolls back without a partial owner release.
- Only after complete successful enumeration, delete the owner's rows and
  commit the transaction. Preserve claims held by other packs, caches or
  user pins. An already-released owner returns an empty inventory.

## Regression and exact source qualification

Test `owner_release_is_bounded_atomic_and_preserves_other_owners` uses a
small injected limit to prove overflow is rejected without changing any
claims, exact-limit release succeeds, unrelated pack and user-pin ownership
survives, and repeated release is idempotent. Invalid owner IDs and limits
fail before a mutation.

Initial source `bbb5d81310d0d9203425807fbf5611b375112fd4`
failed formatting: [Rust CI 40444551781](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40444551781).
Corrected implementation `9a9e1e013bf6016127d9e66f6ee52502db06ee2a`
passed:

- [Rust workspace CI 40444613787](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40444613787) — rustfmt, strict Clippy and workspace tests.
- [Platform shells CI 40444613663](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40444613663) — passed.
- [Android Rust ABI CI 40444613514](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40444613514) — passed.
- Android record codec probe `40444613695` was still running when this document was prepared; do not claim a pass without checking.

## Remaining work and limitations

The returned digests are **advisory, never permission to unlink bytes**.
A separate process can publish unregistered objects before SQLite ownership
registration, and filesystem readers may still hold references. Production
garbage collection needs coordinated installer/reader leases or equivalent
cross-process exclusion, safe deletion and crash recovery. Publisher trust,
complete renderer-required media, atomic pack activation and real desktop/
Android offline acceptance remain outstanding. **STORE-002 stays unchecked.**
