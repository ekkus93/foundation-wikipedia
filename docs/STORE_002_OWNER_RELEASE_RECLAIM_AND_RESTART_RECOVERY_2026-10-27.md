# STORE-002 — Owner release, bounded reclamation and restart recovery

Date: 2026-10-27  
Status: **partial qualification — STORE-002 remains unchecked**

## Implemented functionality

`media_gc::release_owner_and_reclaim_media(store, registry, kind, owner_id, max_candidates)` provides a coordinated lifecycle boundary for releasing every reference owned by one pack, cache entry, or user pin and then reclaiming safely unowned registered objects. The function validates the 1–10,000 candidate budget before any owner mutation, takes an EXCLUSIVE cross-process media lease, invokes the existing bounded transactional `remove_all_for_owner`, and runs the already-proven verified registered-media collector *without re-acquiring its SQLite-backed lease*. Under the same lease, shared-lease readers/installers cannot interleave a dangling reference or re-publication.

The returned `OwnerReleaseGcOutcome { released_objects, cleanup }` intentionally separates the committed SQLite owner release from a later collector error. The outer `Err` means release did not commit; a successful outer `Ok` can contain `cleanup: Err(...)` because the owner was released but the remaining bytes were not necessarily reclaimed. This is essential for a UI/job controller: retry a cleanup sweep, **not** the ownership-release side effect. No candidate digest list is granted standalone unlink authority. Other packs, caches and saved user pins keep the shared content.

A second test specifically exercises *on-disk SQLite restart recovery* rather than only in-memory registration. Three unowned registered media objects and one user-pinned object are created; an injected unlink error interrupts the collector after SQLite commits removal of the three unowned registrations and after its first successful file deletion. The registry is dropped and reopened, and the ordinary orphan collector clears the two surviving objects, counts exact reclaimed bytes, preserves the pinned file and verifies a repeat sweep is empty. This models a discrete crash window; no real power is removed and no device storage faults are injected.

## Qualification

Initial lifecycle source `4cacda4a14ae1870d924df218b2bde1892494dfd` failed the formatting gate ([Rust CI 40508774279](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40508774279)). Corrected implementation `7e585de39c542affaaddf60772e236201e60c954` passed [Rust 40508924645](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40508924645), [Platform shells 40508925081](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40508925081) and [Android ABI 40508924775](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40508924775).

The persistent-reopen regression initially failed rustfmt at `29f8c8ffc132f048a7351344abc72d9500f0d27c` ([Rust 40509495367](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40509495367)). Corrected exact source SHA `6a7945dcd129047a84056e19fc42875175b4770b` passed [Rust workspace CI 40509695405](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40509695405), [Platform shells CI 40509695430](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40509695430), and [Android Rust ABI CI 40509695363](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40509695363). [Android record codec probe 40509695369](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40509695369) remained running when this was recorded.

## Remaining risks and next work

- Connect the API to actual pack/cache/pin uninstall workflows and controlled GC job scheduling; do not claim complete application integration.
- Qualify disk-full and real process-kill/power-loss scenarios around SQLite commit and directory fsync on persistent supported storage and Android.
- The media lease is advisory for **cooperating** clients. Direct unleased registry writers and hostile directory replacement are not generally prevented; data directories must not be untrusted writable storage.
- External authenticated source/attribution, a complete required-offline-visual manifest, atomic pack activation and real-device acceptance remain upstream acceptance gates.

**Do not mark STORE-002 complete.**
