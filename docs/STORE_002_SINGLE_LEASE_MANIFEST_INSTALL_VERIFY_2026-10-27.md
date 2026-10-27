# STORE-002 — Single-lease bound manifest installation and verification

**Date:** 2026-10-27  
**Scope:** Rust media installation, owner registry and cross-process lease  
**Status:** Partial qualification; STORE-002 remains unchecked

## Problem

The production-facing `install_bound_manifest_media_batch` previously called the root-bound batch installer, which acquired and released a SHARED SQLite-backed media lease after registering ownership. It then called the root-bound required-media verifier, which acquired a **different** SHARED lease. A cooperating EXCLUSIVE collector or owner-release operation could acquire the lease between those two calls, remove the newly registered owner's claims, and reclaim its files before post-install verification. A failed install result after a committed owner transaction was also possible.

## Implementation

The manifest-aware installer now obtains **one SHARED lease before the owner preflight** and keeps it alive through root-binding check, manifest/owner consistency check, staged content-addressed publication, atomic SQLite owner registration, and final exact inventory/streamed SHA-256 verification. Internal `media_install::install_owned_media_batch_unlocked` and `verify_required_owner_media_unlocked` helpers assume their caller already holds the lease, preventing nested SQLite lock acquisition. The public root-bound and unbound lower-level entry points retain their existing behavior; unbound manifest installation remains for controlled fixtures/migration, not production use.

The regression invokes a private test seam immediately after the ownership transaction and before verification. A second SQLite connection tries `BEGIN EXCLUSIVE` with a zero busy timeout against `.media-lease.sqlite` and is rejected. After installation returns, the same operation succeeds; exact owner inventory and bytes still verify. This establishes deterministic **cooperating** GC exclusion at the former inter-lease gap, not a real process-kill or power-loss guarantee.

## Verification

Source `c6aff1c315e5839ac18a993077925df35a50b975` failed formatting ([Rust CI 40595564652](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40595564652)); `e06fe2c377b7abc1592864ed9f20161e904a37ef` failed Clippy and missing test import ([Rust CI 40595642404](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40595642404)); `f7a95644423c0a0134c2a1818e9a073b1b20a2f2` failed formatting ([Rust CI 40595761956](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40595761956)). Final corrected code SHA **`2581cba5f94bb82a0c0085a9fc8ab268a23a8715`** passed:

- [Rust workspace CI 40595843641](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40595843641) — rustfmt, strict Clippy and tests.
- [Platform shells CI 40595843793](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40595843793).
- [Android Rust ABI CI 40595843726](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40595843726).

[Android record codec probe 40595843557](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40595843557) remained in progress at documentation.

## Outstanding

- Actual pack/catalog/snapshot activation and uninstall lifecycle integration, plus installed-pack ownership durability across restarts.
- Independent manifest publisher authentication, exact required-visual completeness, and attribution/license acceptance.
- Physical power-loss, disk-full, device and real timer qualification.
- Hostile same-user filesystem/direct SQLite mutation remains outside the cooperative lease protocol.

**Do not check STORE-002 complete.**
