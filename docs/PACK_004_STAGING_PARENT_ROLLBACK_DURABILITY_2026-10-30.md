# PACK-004 — Durable parent sync after failed provisional staging publication

**Date:** 2026-10-30  
**Scope:** `crates/wiki-pack-format/src/stage.rs`  
**Qualified source SHA:** `76013345cc90362a4a3f493311466efa534bd7f3`

## Problem and correction

The unsigned provisional `.wpack` stager previously removed a newly created staged tree if its parent-directory publication sync failed, but did not sync the parent directory **after the removal**. Consequently, an error could be reported even though the rollback removal was not durably recorded across a crash.

`finalize_staged_tree` now attempts the parent sync, removes the staged tree on publication-sync failure, and performs a second parent sync to make the rollback durable. If both cleanup operations succeed, it returns the original publication-sync error. If tree removal or rollback synchronization fails, the returned error explicitly identifies the rollback failure and retains the original publication failure context. This is a fail-closed local staging change, not an installation or publisher-authentication mechanism.

## Regression evidence

- `failed_parent_durability_sync_rolls_back_new_staging_tree`: injects a publication sync failure, asserts the tree has already been removed at the second parent sync, asserts exactly two sync attempts, and preserves the original error.
- `failed_rollback_parent_sync_is_reported_separately`: injects both publication and rollback sync failures, verifies the returned error contains both failure contexts, and verifies the tree was removed.
- Existing pack staging, unseekable-stream, input-budget, transcript-integrity, and trust-boundary tests remain in the Rust workspace suite.

An initial source commit `0bd3230204e3f32fa8d8cbce90c709104ddb4cb5` failed the Rust formatting gate ([run 41125599551](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41125599551)). The corrected exact-head source `76013345cc90362a4a3f493311466efa534bd7f3` passed:

- [Rust workspace CI 41125684582](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41125684582)
- [Platform shells CI 41125684604](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41125684604)
- [Android Rust ABI qualification 41125684612](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41125684612)

## Remaining acceptance

PACK-004 and TRUST-003 are **not complete**. This change does not provide signed publisher authentication, complete required offline visual assets, production pack activation, atomic installed-state rollback, power-loss fault injection on real storage, or physical Android qualification. The parent directory must remain exclusively controlled by the caller during staging.
