# STORE-002 — Lease-coordinated media GC and orphan recovery

Date: 2026-10-27  
Status: **partial; STORE-002 remains unchecked**

## Implementation

- `media_gc::collect_unowned_registered_media`: hold a cross-process EXCLUSIVE SQLite-backed media lease; begin SQLite IMMEDIATE transaction; enumerate a bounded, deterministic unowned registry inventory; validate SHA-256 and registered sizes while the write reservation is held; transactionally delete only objects with zero pack/cache/user-pin owners (attribution cascades); then reverify and unlink the corresponding files, syncing their digest-prefix directories. Registered rows are committed **before** file unlink: an interruption in between can leave a safe, unregistered byte orphan, not a missing still-registered file.
- `MediaRegistry::commit_verified_unowned_media`: internal transaction primitive that rolls back the entire batch if any candidate fails validation, is corrupt, is over the limit, or acquires a new owner before reservation. It returns the newly unregistered digests only after transaction commit.
- `media_gc::collect_unregistered_media_orphans`: hold the same EXCLUSIVE lease, enumerate canonical published digest filenames, reserve SQLite IMMEDIATE to filter only those still lacking any registered row, verify every selected object *before* unlink, and delete/sync while the reservation remains held. Does not touch registered-but-unowned bytes or interrupted publication staging. Repeated sweeps are idempotent.
- `MediaObjectStore::unlink_verified_unlocked` is crate-private and requires an EXCLUSIVE lease from its caller; it rechecks on-disk SHA-256 and size and syncs the containing directory after unlink.
- Media scan now recognizes exactly `.media-lease.sqlite-journal` alongside `.media-lease.sqlite` when an exclusive SQLite DELETE-journal lease is held. Nonregular aliases and hardlinks remain rejected; arbitrary names, SQLite WAL/SHM files, and unexpected subdirectories still fail closed.

## Synthetic qualification

Regression tests cover preserved pack/cache/user-pin owners, release only after final owner removal, successful registered reclamation and idempotence, precommit corruption causing a full registration rollback, injected post-SQLite-commit/unlink failure preserving recoverable unregistered bytes, a subsequent recovery sweep, independently orphaned install bytes, retained interrupted staging, corrupted orphan preflight blocking all deletes, and inventory scanning while EXCLUSIVE lease journal artifacts exist.

Initial registered-GC commit `079ee92eebab24ab9f6f5eeb700f1a24a478a4fd` failed rustfmt, and `31963d44053d7c033d4cac2e7ba05da37f0c6033` failed strict Clippy; corrected registered implementation `4e8da974853b5b8d507b005adf5a08980a7ba8eb` passed Rust CI 40483131268 and Android ABI 40483131159. Orphan recovery at `626a00ea372aee161bf505b3ae022226725e18fe` initially failed formatting, and `ce101f4199566b5d42524cbf129ed9a0cc74a6c0` exposed the exclusive-lease journal scanner bug in tests. Formatting then corrected at `6596ffda7d33131239adcce3fad9bc3b60205fdd`, passing:

- [Rust workspace CI 40484223165](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40484223165)
- [Platform shells CI 40484223352](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40484223352)
- [Android Rust ABI CI 40484223170](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40484223170)
- [Android record codec probe 40484223318](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40484223318) still running at qualification.

## Boundaries and follow-up

Only filesystem actors following the existing cross-process media-lease protocol are coordinated. Direct unleased SQLite writes, external hostile file/directory replacement, or a media root writable by untrusted processes violate these assumptions. Unlink errors **after** committing SQLite state can leave unregistered orphans; retry via the separate unregistered sweep, not by blindly unlinking stale candidates. Neither sweep is automatically scheduled or coupled to the uninstall lifecycle. Add explicit killpoint/power-loss/disk-full fault injection, lease contention stress tests, user-visible reclaim diagnostics, and safe lifecycle integration. This does not establish Wikimedia source authenticity, complete required offline visuals, publisher signatures, pack activation, or real-device acceptance. **Do not mark STORE-002 complete.**
