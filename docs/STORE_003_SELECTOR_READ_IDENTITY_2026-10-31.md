# STORE-003 — Stable active-selector read identity qualification

**Date:** 2026-10-31  
**Repository:** `ekkus93/foundation-wikipedia`  
**Qualified source SHA:** `9d9d32d4d9bab1e7ffe0847dc42cebbe64bdf19a`

## Problem

The snapshot selector reader already rejected an initially symlinked `current` file and hardlinks, and bounded the selector bytes. But after reading the open descriptor it only checked its length and link count, not that the `current` pathname still referred to the inspected inode. Replacing `current` after reading with another file containing the same snapshot ID and length could return an apparently valid selection from a descriptor that was no longer authoritative. This matters especially when higher-level callers use the selection to decide which pack-owned media may be released.

## Implementation

- Added a shared, private `same_pointer_snapshot` guard in `crates/wiki-store/src/activation.rs`, comparing regular-file type and length. On Unix, it also checks device, inode, single-link ownership, and nanosecond modification/change timestamps; on other platforms it compares available creation/modification metadata.
- `ActiveSnapshotStore::current` now checks identity at path inspection versus opened descriptor, opened descriptor versus post-read descriptor, and post-read descriptor versus the final `current` path. Any detectable change is reported as `ActivationError::CorruptCurrentPointer`.
- Added a private post-read test seam and deterministic Unix regressions for a same-bytes/same-length inode replacement and a symlink substitution after reading. Both reject the replaced pointer, while a stable restored pointer remains readable.

## Exact-head verification

- Initial source `f1df353f697046c22a23e421e36d8f2ea3483b8a`: [Rust workspace 41235351870](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41235351870) failed the formatting check.
- Corrected source **`9d9d32d4d9bab1e7ffe0847dc42cebbe64bdf19a`**: [Rust workspace 41235397523](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41235397523) **passed**; [Platform shells 41235397522](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41235397522) **passed**; [Android Rust ABI 41235397569](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41235397569) **passed**. [Android record codec emulator 41235397557](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41235397557) was pending at the time of writing.

## Boundaries

This is a race-detection improvement, **not** a descriptor-relative atomic open or a lock against a malicious writer. The snapshot root must remain exclusively controlled by the application. This does not validate publisher identity, installed article/shard/media completeness, user-state isolation, or power-loss recovery. Physical device, interruption and cross-platform acceptance remain. **STORE-003 remains unchecked.**
