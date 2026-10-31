# STORE-003 — Snapshot selector durability rollback qualification

**Date:** 2026-10-31  
**Repository:** `ekkus93/foundation-wikipedia`  
**Qualified code SHA:** `97902beb5fe06d98479c2352d3f7aaaa2d44e0b9`

## Issue

The snapshot activation primitive writes a synced private selector, atomically renames it over `current`, then syncs the parent directory. If that last fsync fails, the old implementation returned an error even though the new snapshot was already selected in the running process. Callers could interpret the failed activation as preserving the previous snapshot and incorrectly manage its media ownership.

## Implementation

- Validate the previous `current` selector before publication, rejecting corrupt pointers instead of replacing them without a recoverable predecessor.
- When there is a previous selector, create a separate `create_new` private rollback file, write the previous snapshot ID, sync its contents, then sync the parent directory before publishing the replacement.
- On failure to sync the parent after the new selector's rename, restore the previous selector with an atomic rename, or remove the newly created selector if there was no previous one. Sync the parent directory again to confirm rollback durability.
- Report an ordinary I/O failure only after a confirmed rollback. If the restore operation or its durability sync fails, return `ActivationError::RollbackFailed { commit_error, rollback_error }` to distinguish an uncertain on-disk outcome.
- Leave stale private selector files non-authoritative. A failed switch attempts to clean temporary files without truncating or adopting abandoned files from earlier interruptions.

## Regression evidence

Deterministic unit tests inject a directory fsync failure after publication with both a previously active snapshot and a previously uninitialized store. They assert the old or absent selector is restored, the correct number of sync attempts is made, and private temporaries are cleaned. A separate test injects a second failure during rollback sync and asserts the explicit uncertain-durability error. A corrupt existing pointer cannot be overwritten by ordinary activation.

The initial code SHA `be45216047c5b5762c4673453fb25a0d1e0dcbd6` failed rustfmt ([Rust workspace 41231340436](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41231340436)). The formatted follow-up `e43499461644afd5a9d939387cdb36f131ac9560` inadvertently omitted the source-file tail during a bounded read and failed parsing ([Rust workspace 41231405193](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41231405193)). The complete tail was restored in **`97902beb5fe06d98479c2352d3f7aaaa2d44e0b9`**: [Rust workspace 41231486217](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41231486217) **passed**, [Platform shells 41231486179](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41231486179) **passed**, [Android Rust ABI 41231486177](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41231486177) **passed**. [Android record codec probe 41231486247](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41231486247) was still running when this qualification was recorded.

## Boundaries and remaining work

This is a deterministic failure-path regression, not proof of physical power-loss survival. The root must be exclusively controlled by the snapshot manager; concurrent noncooperating writers can still invalidate assumptions between validation and rename. The current selector primitive does not independently verify publisher authenticity, article/catalog/shard integrity, required offline media completeness, cross-platform filesystem durability, or end-to-end install/rollback UI behavior. Mutable user-state separation and actual disk-full/kill/power-loss acceptance remain. **STORE-003 stays unchecked.**
