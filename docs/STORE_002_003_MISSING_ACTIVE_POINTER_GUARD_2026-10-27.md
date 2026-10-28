# STORE-002 / STORE-003 — Missing active pointer fails closed on pack release

**Date:** 2026-10-27 (America/Los_Angeles)  
**Repository:** `ekkus93/foundation-wikipedia`  
**Exact source commit:** `fffc0611365a943a7ed7c934f69093386165c213`

## Problem

`release_inactive_snapshot_pack_and_reclaim_media` guarded the selected active snapshot, but accepted `ActiveSnapshotStore::current() == None` as authorization to remove any named pack owner. A fresh store or one that lost its active pointer could therefore revoke the only previously usable pack's media ownership and allow bounded garbage collection. A missing pointer is not evidence that the pack has been safely deactivated.

## Implementation

`crates/wiki-store/src/media_gc.rs` now requires `Some(active_snapshot_id)` before checking the requested inactive ID. A missing pointer returns `InactivePackReleaseError::MissingActiveSnapshot` before `remove_all_for_owner` or any GC commit. A malformed pointer continues to return an explicit `ActivationError`; the active snapshot continues to return `ActiveSnapshot`.

The opt-in API continues to hold an exclusive cross-process media lease spanning binding validation, pointer read, owner release and GC. The check does not make unleased legacy APIs safe, authenticate a pack, or implement rollback retention.

## Regression evidence

The existing disk-backed `inactive_pack_release_refuses_active_and_preserves_new_active_media` regression now asserts:

1. A root-bound registry containing installed `old`/ `new` assets but no active pointer must reject `old` owner release; the registry claim, SHA-256 verified object bytes and `None` current pointer remain unchanged.
2. A valid media-gated activation of `old` still refuses active-owner release.
3. Removing the active pointer simulates loss of selector state. The same release must fail closed without revoking `old` ownership or deleting its verified bytes.
4. Restoring the pointer through verified-media activation permits the existing successful `old`→`new` switch and subsequent inactive `old` reclamation, while preserving `new` media.

The initial source SHA `708fd37a78a2298f3523ce9c7cfdc872bfeee5af` failed formatting [Rust CI 40625813662](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40625813662). The corrected exact source commit `fffc0611365a943a7ed7c934f69093386165c213` passed:

- [Rust workspace 40625889936](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40625889936)
- [Platform shells 40625889909](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40625889909)
- [Android Rust ABI qualification 40625890011](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40625890011)

The [Android codec probe 40625889877](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40625889877) initially failed at the pinned Android NDK installer (`Error on ZipFile unknown archive`), before running code tests; retry requested. Do not interpret an accepted retry as a passing emulator test.

## Limits and remaining acceptance

Both STORE-002 and STORE-003 remain unchecked. This is a deliberately conservative opt-in release boundary, not a complete pack lifecycle. Authenticated manifests, rollback retention, direct writer protections, physical crash/disk-full tests, complete media inclusion, and desktop/Android end-to-end qualification remain outstanding. Old direct release APIs and unleased pointer mutation must not be used to bypass the production gate.
