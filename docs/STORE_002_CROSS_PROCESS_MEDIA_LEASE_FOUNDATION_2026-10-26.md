# STORE-002 — Cross-process media lease foundation (2026-10-26)

**Status:** Partial implementation. Production garbage collection remains disabled.

## Implementation

Added `wiki-store::media_lease::MediaLeaseCoordinator` with SHARED and
EXCLUSIVE SQLite rollback-journal transactions on a private
`.media-lease.sqlite` inside the media object root. A SHARED lease
executes a real SQLite read to hold the lock; EXCLUSIVE waits for
participating shared holders. Closing the SQLite connection releases the
OS lock, including on process termination. WAL is rejected because
readers would not exclude writers. The lease database path reuses the
registry's symlink, traversal and Unix hardlink preflight; the media
object scanner skips only that exact regular-file coordination name.

`install_owned_media` and `install_owned_media_batch` acquire a SHARED
lease **before publishing bytes**, retaining it until the final owner
registration commits. The read-only
`plan_unowned_media_gc_with_registry_reservation` acquires an EXCLUSIVE
lease **before** its SQLite IMMEDIATE registry reservation, then rehashes
candidate bytes with bounded streaming SHA-256. This consistent lock
ordering avoids the cooperating installer/GC race where published bytes
exist before registry ownership is recorded. No deletion or metadata
removal is performed by these functions.

## Regression coverage

- Two simultaneous SHARED leases block an EXCLUSIVE contender until both
  are released; an EXCLUSIVE holder blocks a new SHARED transaction.
- A real installer running on another thread cannot publish/register
  while an EXCLUSIVE lease is held; it completes and verifies its
  owner-scoped media after release.
- Coordination files are private on Unix and cannot be hidden from
  inventory as symlinks or hardlink aliases.
- Existing object-store and registry regressions remain in Rust CI.

## Exact-source CI

The first primitive commit `acf3b59a6c0d4ad1552dc8b21de5bd6908da4553`
failed rustfmt ([Rust run 40463176553](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40463176553)).
The corrected primitive `e05f369d6bf5a4fe496e0e897506f1c0d96e3779`
and integrated source `5c335d8f60bfba4122c92c7bfbd2983d5a3aee44`
passed Rust workspace, Platform shells and Android Rust ABI qualification.
The first contention-test commit `38e9f544b5b64d9725df1d794c469ca61b1fd368`
failed rustfmt ([Rust run 40463540692](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40463540692)).
Corrected exact code `1e990f9a222aa1602c88cee72a6a9adb0b13958e`
passed [Rust workspace 40463601651](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40463601651),
[Platform shells 40463601680](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40463601680),
and [Android Rust ABI 40463601639](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40463601639).
[Android record codec probe 40463601637](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40463601637)
was still running when this report was written.

## Remaining safety work

This is **not** a universal media lease yet. Direct
`MediaObjectStore::{store_bytes, read_verified, verify_object_size,
published_object_digests}` callers and other reader/verification
operations do not acquire the shared lease. External registry writers
may bypass it. A production collector still needs mandatory participation
across all readers and publishers, coordinated filesystem unlink with
fresh ownership checks, crash-safe tombstones and replay, and
cross-process crash/fault-injection qualification. No object deletion
is authorized, and STORE-002 remains unchecked.
