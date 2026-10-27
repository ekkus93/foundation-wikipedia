# STORE-002 — Lease-scoped root-binding checks for destructive operations

Date: 2026-10-27  
Status: **partial qualification — STORE-002 remains unchecked**

## Scope

The existing persistent `MediaRegistry` binding records the canonical media root and Unix device/inode identity. Previously, the CLI checked that binding **before** entering the exclusive maintenance lease, while the low-level owner-release API had no root-binding precondition. That preflight is insufficient as a production safety boundary because the registry/root may be changed between the check and exclusive lock acquisition.

Added `media_gc::run_bound_media_maintenance`: it validates the candidate/scan limits, obtains the EXCLUSIVE media lease, then rechecks `require_media_root_binding(store.root())` **under that lease** before running the existing registered-unowned and orphan-recovery phases. `tools/wiki-pack/src/media_maintenance.rs` now routes the destructive `--confirm-delete` mode through this bound entry point; the prior check remains for read-only `--dry-run`. The unbound low-level `run_bounded_media_maintenance` remains available for internal fixtures and callers, but is not an acceptable production deletion entry point without an independently enforced binding.

Added `media_gc::release_bound_owner_and_reclaim_media` for future pack/cache/user-pin lifecycle integration. It performs the same under-lease root-binding precondition before the owner-release SQLite transaction, and then uses the existing bounded registered GC. The original unbound release API remains for controlled test fixtures. Application-level uninstall workflows must choose the bound variant; no claim of complete UI wiring is made.

Regression tests verify an unbound registry is rejected with its bytes and SQLite rows unchanged, a bound registry presented with a different root is rejected without deletion or owner revocation, a matching root reclaims unowned media or releases a user pin correctly, and repeated maintenance is idempotent.

## Exact-head CI evidence

- Initial maintenance guard SHA `7ecfabdfa33a80ba4078a56ffe912b4dd0016444`: [Rust workspace 40537648886](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40537648886), [Platform shells 40537649100](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40537649100), [Android Rust ABI 40537648932](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40537648932) all passed.
- Final bound owner-release SHA `f22ab5cc8276dac566dd5e6aac97df53de2bc1a3`: [Rust workspace 40538178616](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40538178616), [Platform shells 40538178563](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40538178563), [Android Rust ABI 40538178697](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40538178697) all passed. [Android record codec probe 40538178736](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40538178736) was still in progress at documentation.

## Boundaries and next work

This coordination only excludes **cooperating** installers, readers and GC clients. It cannot prevent an adversarial process with direct SQLite write access, arbitrary media-root replacement, or host-level disk corruption. A lease-scoped binding check is a safety guard, not source/publisher authentication. Remaining STORE-002 work includes scheduled bounded maintenance, full production pack/cache/pin lifecycle integration, disk-full/real process-kill/power-loss validation, upstream attribution/license/authenticity, and desktop/Android device acceptance.

**STORE-002 must remain unchecked.**
