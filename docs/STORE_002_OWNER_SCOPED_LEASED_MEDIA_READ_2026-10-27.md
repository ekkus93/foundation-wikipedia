# STORE-002 — Owner-scoped leased media read (2026-10-27)

**Status:** Partial STORE-002 implementation. No production media deletion is enabled.

## Implementation

Added `wiki_store::media_install::read_verified_owned_media` and
`VerifiedOwnedMedia`. The reader accepts an explicit owner kind, owner ID,
and content digest. It acquires a SHARED cross-process media lease **before**
checking the owner's registered digest and holds it until the verified bytes
and attribution notices have been read. Unknown or unrelated owner claims
return `None`. An owned object must have valid bounded attribution and
a matching registered size; missing, tampered, or unsafe content fails closed.

The implementation calls `MediaObjectStore::read_verified_unlocked` while
holding the lease, rather than nesting `read_verified` and risking a deadlock
against a pending EXCLUSIVE collector. The returned object contains exactly
the verified bytes and complete validated attribution notices.

## Automated regression coverage

- Correct pack ownership, exact content, and attribution; unrelated pack
  and user-pin owners cannot read through another owner's claim.
- Attribution corruption (control characters) fails without exposing bytes.
- Registered byte-size corruption fails after verifying the actual object.
- Same-size on-disk tampering fails SHA-256 verification.
- Revoked ownership returns `None`.
- A second thread using an independent on-disk SQLite registry connection
  blocks on an EXCLUSIVE media lease and completes only after it is released.

## Exact-commit CI

Initial code SHA `9755ddc174b14466f0fecf0e868e471d9c12166f`
failed the Rust formatting gate ([run 40464939890](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40464939890)).
Corrected implementation SHA `cbb33d9dca4e8508eb2260f40fab60a0fcfaa502`
passed [Rust workspace CI 40465035444](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40465035444),
[Platform shells CI 40465035340](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40465035340),
and [Android Rust ABI CI 40465035384](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40465035384).
[Android record codec probe 40465035439](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40465035439)
was still in progress when this note was written.

## Remaining work and security boundary

The lease protects cooperating file operations against a concurrent
EXCLUSIVE collector; it does **not** freeze the SQLite owner table. An
independent connection may revoke or change an owner during the read.
This API is not an access-control boundary or publisher authenticity proof.
No snapshot activation, complete visual coverage, signed pack provenance,
or safe filesystem deletion is implied.

STORE-002 still requires coordinated owner revocation/GC semantics,
crash-safe deletion and tombstone recovery, staging recovery and fault
injection, and qualification on real desktop/Android installations.
