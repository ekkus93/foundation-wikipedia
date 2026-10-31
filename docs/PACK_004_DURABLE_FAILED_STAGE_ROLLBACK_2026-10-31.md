# PACK-004 — Durable rollback of incomplete unsigned staging trees

**Date:** 2026-10-31  
**Implementation SHA:** `72e663db61d7e3301c26a741b95e753ebba680d9`  
**Scope:** `crates/wiki-pack-format/src/stage.rs`; unsigned staging only.

## Failure mode

The seekable two-pass importer verifies the complete envelope before creating its destination directory. A mutable seekable source can nonetheless truncate or fail during the second pass, after the importer has created the private stage and written partial object bytes. Previously, the failure branch removed that tree but did **not** sync the staging parent after removal. After a sudden crash, the directory entry removal might not be durable, allowing an incomplete staging tree to reappear. Likewise, a successful nonseekable stage whose temporary spool cleanup failed removed its new stage without a parent durability barrier.

## Implementation

The `rollback_incomplete_stage` helper removes the newly created tree, then syncs the parent directory before returning the original staging failure. If removal or the parent sync fails, the returned error includes both the original failure and the rollback failure, so callers cannot mistake an incomplete cleanup for success. The helper is used for second-pass staging failures and for post-stage spool-cleanup failures. It does not overwrite or remove a preexisting stage: destination creation remains exclusive.

## Automated qualification

- Synthetic seekable input passes complete first-pass envelope verification, then truncates the second-pass payload after the canonical transcript and first object byte. The importer returns `UnexpectedEof` and leaves no staged directory.
- Injected parent-directory sync failure after successful stage removal is reported explicitly alongside the original staging error. The stage is absent from the filesystem after the injection.
- Existing seekable/nonseekable staging, input budgets, transcript-swap, finalization sync, and rollback tests remain in the Rust workspace suite.

The first source commit `fcd678b4d0aba117750952fbd6babc64f8023c1a` failed rustfmt ([Rust 41226724926](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41226724926)); corrected source **`72e663db61d7e3301c26a741b95e753ebba680d9`** passed [Rust workspace 41227006225](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41227006225), [Platform shells 41227006247](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41227006247), and [Android Rust ABI qualification 41227006248](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41227006248).

## Boundaries

This improves crash-conscious cleanup but is **not** a real power-loss or filesystem durability experiment. It assumes the caller owns the private staging parent and no adversary races filesystem operations. It does not implement authenticated publisher signing, mandatory offline visuals and attribution, atomic installed-pack activation, Android SAF integration, or physical-device acceptance. **PACK-004 remains unchecked.**
