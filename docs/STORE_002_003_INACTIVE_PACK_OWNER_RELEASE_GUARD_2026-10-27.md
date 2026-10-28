# STORE-002 / STORE-003 — Inactive pack owner release guard (2026-10-27)

## Verified source and scope

Source SHA: `17da3754fb2fa489456de45613084c26902316fd`.

`crates/wiki-store/src/media_gc.rs` adds `release_inactive_snapshot_pack_and_reclaim_media(snapshots, store, registry, snapshot_id, max_candidates)`. This opts pack removal into an explicit active-snapshot safety gate. Before revoking ownership, it validates bounded GC limits, takes an EXCLUSIVE cross-process media lease, verifies the persisted registry-to-media-root binding, and reads the crash-conscious active snapshot pointer using `ActiveSnapshotStore::current`. If `snapshot_id` matches the active pointer, it returns `InactivePackReleaseError::ActiveSnapshot` without touching ownership or bytes. Invalid/corrupt active pointers also cause a typed error and no owner release. For an inactive pack, ownership revocation and bounded registered-unowned GC occur under the same exclusive lease. Callers MUST inspect the returned inner `cleanup` result because a successful owner-release transaction may be followed by a recoverable GC error.

The paired `switch_to_prevalidated_with_bound_pack_media` activation API holds the media lease SHARED through verification and pointer commit, so these **cooperating** activation and release entry points cannot interleave the active-pointer read with a switch.

## CI and regression evidence

Disk-backed tests cover two separately owned snapshot figures, root binding, media-verified initial activation, refusal to release an active pack (both owner claims and file remain), bad GC limit rejection, media-verified switch to a new snapshot, and successful subsequent reclaim of the now-inactive old figure while preserving the new active owner, bytes and pointer.

- [Rust workspace 40615833432](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40615833432): passed format, strict Clippy, unit tests and workspace gates.
- [Platform shells 40615833556](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40615833556): passed.
- [Android Rust ABI qualification 40615833443](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40615833443): passed.

Initial formatting failure [40615286239](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40615286239) was corrected. A subsequent bounded file-read rewrite caused accidental source truncation at `8a80902fd99e0573d788d6216ffa5bf1cf9cbff6`, detected by [40615468531](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40615468531), and the missing tail/module closure was restored before the passing source commit. These failed attempts are recorded explicitly; only the passing source head qualifies the implementation.

## Remaining acceptance gaps

This is an opt-in, software-only lifecycle guard, not a complete pack uninstall protocol. Legacy `release_bound_owner_and_reclaim_media` and `switch_to_prevalidated` APIs can bypass the active-owner gate; production application entry points must migrate to the paired gated APIs. The method does not prevent noncooperating direct SQLite/filesystem writes or unleased pointer switches. It intentionally permits deleting an *inactive* pack, which may include a desired rollback snapshot; production callers must enforce a rollback-retention policy. Publisher authentication, independently complete media manifests, crash-recovery orchestration, desktop/Android UI and device acceptance, and physical power-loss/disk-full testing remain outstanding. STORE-002 and STORE-003 remain unchecked.
