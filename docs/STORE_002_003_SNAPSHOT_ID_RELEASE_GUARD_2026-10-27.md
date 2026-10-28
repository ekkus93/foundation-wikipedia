# STORE-002 / STORE-003 — Snapshot ID validation on pack release (2026-10-27)

## Scope

The opt-in `release_inactive_snapshot_pack_and_reclaim_media` path must accept only IDs that the snapshot activation path itself can select. The shared SQLite media registry intentionally accepts a broader grammar for owner IDs, including uppercase names, leading digits and lengths up to 255 bytes. Those are valid registry identities but **not** valid snapshot identities. Previously the inactive-snapshot release entry point could revoke their pack ownership and reclaim their last media bytes even though no such snapshot could ever become active.

The release path now reuses `activation::safe_id` before taking an exclusive media lease or mutating the registry. Invalid IDs return `InactivePackReleaseError::Activation(ActivationError::InvalidSnapshotId)`. The ordinary owner-release API remains unchanged for its separate callers.

## Regression

`activation::interruption_tests::inactive_pack_release_rejects_non_snapshot_owner_ids_without_revocation` creates an active snapshot, binds a persistent registry to a real media root, installs separately attributed media for four registry-legal but snapshot-illegal pack IDs, and attempts each release. Every attempt must fail with the typed invalid-ID error, preserve its SQLite owner claim and keep the exact digest-verified media readable. The active snapshot pointer is unchanged.

## Qualification boundary

This is a narrow input-contract and media-retention safeguard, **not** a full pack installer/uninstaller, rollback-retention mechanism, publisher-authenticated manifest verifier, crash/disk-full fault-injection qualification, or physical Android/desktop acceptance. STORE-002 and STORE-003 remain unchecked. Exact-head CI evidence should be recorded after the source commit.
