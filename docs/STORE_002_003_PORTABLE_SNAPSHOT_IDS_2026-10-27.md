# STORE-002 / STORE-003 — Portable snapshot IDs and safe owner release

**Date:** 2026-10-27 (America/Los_Angeles)  
**Repository:** `ekkus93/foundation-wikipedia`  
**Source commit:** `35b31834b3f5fc56039fd71ba3ce6da52456ddba`

## What changed

The snapshot-directory identity validator in `crates/wiki-store/src/activation.rs` now rejects ASCII Windows device basenames `con`, `prn`, `aux`, `nul`, `com1`–`com9`, and `lpt1`–`lpt9` on all platforms. Existing rules for lowercase ASCII first character, ASCII-safe continuation, and maximum 128 bytes remain. Non-reserved names including `com0`, `com10`, `lpt0`, `lpt10`, `console`, `con-old`, and `aux_2026` remain permitted.

This invariant is shared by snapshot activation and the opt-in inactive-pack media-owner release boundary. It avoids accepting names on Linux/Android that cannot safely name portable snapshot directories on Windows.

## Regression evidence

- New `activation::interruption_tests::snapshot_ids_reject_windows_device_names_before_filesystem_access` verifies all eight representative reserved names are rejected as `InvalidSnapshotId` before accessing a nonexistent snapshot root, while nonreserved lookalikes remain accepted.
- Expanded `inactive_pack_release_rejects_non_snapshot_owner_ids_without_revocation` installs media for the reserved identifiers through the deliberately broader media registry and proves the snapshot-specific release gate preserves pack ownership, verified bytes, and the active snapshot pointer.
- Initial source SHA `5b93b7000e0b3e71c6b4a07773b7421f10081fbf` failed only `cargo fmt --all -- --check` in Rust run 40623742834. The format-only follow-up is exact source SHA `35b31834b3f5fc56039fd71ba3ce6da52456ddba`.

### Exact-source CI

- [Rust workspace 40623878534](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40623878534): passed (formatting, strict Clippy, workspace tests and existing regression gates).
- [Platform shells 40623878478](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40623878478): passed.
- [Android Rust ABI 40623878583](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40623878583): passed.
- [Android record codec probe 40623878430](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40623878430): pending at this documentation snapshot. Not used to claim pass.

## Scope boundary

This guard does **not** authenticate source or pack manifests, make media-file publication crash-atomic, protect uncooperative filesystem or SQLite writers, define rollback-retention policy, integrate a complete pack install/uninstall, or qualify real-device/physical power-loss acceptance. Both STORE-002 and STORE-003 remain unchecked. The tests do not exercise Windows hardware; the cross-platform reserved-name policy is enforced by the shared Rust validator.
