# STORE-002 — Atomic single-media ownership registration

**Date:** 2026-10-27  
**Implementation SHA:** `832c739c46777281846b4ff7d32638209522e8c2`  
**Scope:** `crates/wiki-store/src/media_ownership.rs`, `crates/wiki-store/src/media_install.rs`

## Failure mode

The single-object installer published verified content-addressed bytes, called `register_verified_bytes` (committing a new digest and attribution notice), and only then called `add_owner` in a separate SQLite write. If the owner insert failed because of a capacity constraint, database error, or interruption, the registry could retain newly registered metadata without any owner. That was recoverable by bounded GC but needlessly widened the failure window and could complicate attribution/reconciliation.

## Change

`MediaRegistry::register_verified_owned_single` validates owner ID, byte limits and attribution, derives SHA-256 from supplied bytes, reserves the SQLite writer with an IMMEDIATE transaction, checks size and MIME consistency, enforces attribution/owner fanout ceilings, inserts object/notice/owner rows and commits **once**. Both unbound fixture/migration and root-bound production single-object installers use this atomic registry operation. The existing shared media lease continues to cover root-binding validation, byte publication and registry commit. Single-object installs remain *incremental*; immutable exact-inventory pack installs must still use the batch/manifest APIs.

## Regression and exact-head evidence

The disk-backed `bound_single_install_rolls_back_metadata_when_owner_insert_fails` regression first installs two different digests for one pack owner, then creates a SQLite trigger that aborts a third object's owner insert **after** its digest and notice were inserted within the transaction. The call returns an SQLite error, object/notice/owner row counts remain unchanged, the denied owner has no claims, and the already published third digest is still visible as an unregistered orphan. After removing the trigger, a retry succeeds and all three row counts increase exactly once. This tests transactional rollback without claiming crash-atomic filesystem publication.

The initial source `c1f42883fc9681509193708926296463ee41268d` failed Rust formatting ([run 40609787813](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40609787813)). Corrected source `832c739c46777281846b4ff7d32638209522e8c2` passed [Rust workspace 40609876376](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40609876376), [Platform shells 40609876205](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40609876205), and [Android Rust ABI 40609876223](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40609876223). [Android record codec probe 40609876213](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40609876213) remained in progress at this observation.

## Boundaries

Filesystem bytes are intentionally published before the SQLite transaction and may remain orphaned on failure; safe maintenance must reclaim them under the existing root-binding/lease checks. This does **not** establish a crash-atomic pack installation, authenticate source/media manifests, guarantee offline visual completeness, defend against hostile same-user writers, or qualify disk-full/power-loss or desktop/Android device acceptance. **STORE-002 stays unchecked.**
