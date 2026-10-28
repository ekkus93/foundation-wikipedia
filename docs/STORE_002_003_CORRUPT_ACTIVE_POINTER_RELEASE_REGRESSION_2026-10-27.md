# STORE-002 / STORE-003 — Corrupt active snapshot release regression (2026-10-27)

## Scope

This is a focused regression for the opt-in active-pack media ownership protection introduced in `media_gc::release_inactive_snapshot_pack_and_reclaim_media`. It is **not** completion of either canonical task.

The new disk-backed test `activation::interruption_tests::inactive_pack_release_fails_closed_for_corrupt_active_pointer_or_manifest` in `crates/wiki-store/src/activation.rs` installs an attributed content-addressed image for a pack, binds the registry to its media root, and activates that pack through the SHARED-lease required-media gate. It then deliberately corrupts the current snapshot pointer and, separately, removes the current snapshot manifest. Each invalid active-state read must return `InactivePackReleaseError::Activation(ActivationError::CorruptCurrentPointer)`, preserve the pack's SQLite owner claim, and leave its digest-verified media readable. After restoring the manifest, a valid active pointer must return `ActiveSnapshot` rather than revoke the active pack.

## Exact-source evidence

- Initial test commit `3d07e58af7053fc1c9223afa797b1bfaf0a5367f`: [Rust CI 40618554259](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40618554259) failed the formatter.
- Formatting repair `5f08f77918d20090385e19033d83811a6db05577`: [Rust CI 40618615478](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40618615478) passed formatting but failed Rust lint compilation because `MediaRegistry::open` requires `&Path`.
- Corrected source commit `a9603a2135c719ac3d9ecaf15ee7620b9ffa5ef8`: [Rust workspace CI 40618690228](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40618690228), [Platform shells CI 40618690360](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40618690360), and [Android Rust ABI CI 40618690346](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40618690346) passed. [Android record codec probe 40618690278](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40618690278) was still running when this was written.
- The regression runs within the existing `cargo test --workspace` suite in Rust workspace CI; no independent physical-device or disk-full test is claimed.

## Remaining qualification

The snapshot lifecycle still requires an integrated installer/uninstaller and rollback-retention policy, manifest and catalog/shard provenance verification, explicit protection against legacy unleased callers, crash/disk-full fault injection and real desktop/Android offline media acceptance. Both STORE-002 and STORE-003 remain unchecked.
