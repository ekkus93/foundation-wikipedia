# STORE-002 — Single-lease media inventory verification (2026-10-27)

**Status:** Partial STORE-002 implementation. Production garbage collection remains disabled.

## Changes

`media_inventory::verify_declared_media` validates a manifest's bounds,
unique digests, size claims and total byte budget before acquiring one SHARED
cross-process media lease. The lease spans every streaming SHA-256 and size
check, so a cooperating EXCLUSIVE collector cannot remove an earlier object
between verification of later manifest members. An empty valid inventory
returns zero without opening a lease database.

The internal `verify_declared_media_unlocked` shares the exact same
manifest validation and streaming verification logic but requires the caller
to hold a lease. `media_install::verify_registered_owner_bytes` and
`verify_required_owner_media` acquire a SHARED lease **before** querying
owner inventory and keep it through complete digest/size comparison and all
file verification. They use the unlocked helper to avoid recursively
acquiring SQLite locks, which could deadlock with a waiting collector.

## Tests

Added a two-object manifest regression that holds an EXCLUSIVE lease,
starts an independent worker performing `verify_declared_media`, confirms
it cannot finish while the lease is held, and confirms the correct total
after release. Existing tests continue to cover duplicate digests,
impossible sizes, streaming multi-megabyte objects, same-size byte tampering,
and external hardlink aliases on Unix.

## Exact-commit CI

The initial source SHA `a904e77b5d0148903a77743bd66f1c30f2f14b4c`
failed the Rust formatting gate
([40465438492](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40465438492)).
An intermediate formatting SHA `1d31d523e857809aa08999e5ea94ece3100c1786`
still had an import-layout mismatch
([40465535603](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40465535603)).
The corrected implementation SHA `c20d293394ce2f10d086806181b774641cde96b0`
passed [Rust workspace CI 40465607246](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40465607246),
[Platform shells CI 40465607143](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40465607143),
and [Android Rust ABI CI 40465607272](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40465607272).
[Android record codec probe 40465607206](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40465607206)
was still running when recorded.

## Remaining safety boundaries

The shared lease protects cooperating filesystem operations, not SQLite
ownership stability. Another registry connection may revoke a claim during
verification. A verified inventory does not prove publisher provenance,
complete article visual discovery, or pack activation. Production garbage
collection still needs safe unlink/tombstone coordination, crash recovery,
fault injection, and full reader/installer integration. STORE-002 remains
unchecked.
