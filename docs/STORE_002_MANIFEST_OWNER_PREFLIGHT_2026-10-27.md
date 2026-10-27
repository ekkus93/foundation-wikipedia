# STORE-002 — Manifest owner preflight before media publication

**Date:** 2026-10-27  
**Scope:** `crates/wiki-store/src/media_manifest_install.rs`  
**Implementation SHA:** `c77a07ba8b1f90e3ed443bd6eb494095cdc264cf`

## Problem and change

The manifest-aware installer called `install_owned_media_batch_unlocked` while holding the caller's shared media lease. Unlike the public batch wrapper, that unlocked helper does not validate the owner ID before publishing objects. A malformed pack/cache/pin owner ID could therefore cause content-addressed bytes to be staged, only for the SQLite ownership transaction to reject the invalid ID. The resulting unowned objects were recoverable by maintenance but should never have been published for an invalid request.

`install_manifest_media_batch_with_binding` now calls the same `valid_owner_id` contract used by the public installer **before** it hashes or stages any assets. Invalid owner IDs return `ManifestMediaInstallError::Install(MediaInstallError::Registry(MediaRegistryError::InvalidInput))` without publishing media. The guard applies to both production root-bound and controlled unbound fixture/migration variants. The existing single shared lease still covers the bound install, ownership commit, and post-commit byte verification.

## Regression and exact-head qualification

The disk-backed root-bound manifest regression attempts an install for owner `../invalid`, checks the typed `InvalidInput` error, checks that neither expected digest was published, and checks that the valid pack owner's registry inventory is empty. The same test then verifies correct-root installation, persistent reopen, shared user-pin retention, and conflicting-manifest rejection. Other manifest-install race and single-lease tests remain in the workspace suite.

The initial source SHA `6172c8a300e12fa3d25441f7771b457ef6d52190` failed **format only** in [Rust CI 40603109243](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40603109243). The corrected source SHA `c77a07ba8b1f90e3ed443bd6eb494095cdc264cf` passed [Rust workspace 40603181921](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40603181921), [Platform shells 40603181918](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40603181918), and [Android Rust ABI 40603181917](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40603181917). Android record-codec probe `40603181913` was in progress at documentation.

## Boundaries and remaining work

This closes one input-validation gap only. It does **not** make the publication plus SQLite ownership transaction crash-atomic, eliminate orphan bytes from power loss/disk-full, authenticate the required-media manifest, prove complete article visual dependencies, install/activate a pack, or protect against direct same-UID SQLite/filesystem writers. Production pack/catalog/snapshot lifecycle integration, authenticated provenance, physical fault injection, desktop/Android offline media, and real-device acceptance remain outstanding. Keep **STORE-002 unchecked**.
