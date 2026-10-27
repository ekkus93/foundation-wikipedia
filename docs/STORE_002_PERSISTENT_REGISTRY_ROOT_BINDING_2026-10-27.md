# STORE-002 — Persistent media registry-to-root binding

Date: 2026-10-27  
Status: **scoped qualification; STORE-002 remains unchecked**

## Problem and invariant

An existing, valid SQLite registry is not necessarily the registry for the media-object directory supplied to the maintenance CLI. Without binding, a mistaken registry path can make legitimate published files appear unregistered and eligible for deletion. A schema check alone cannot distinguish that mismatch.

The registry now stores a durable singleton `media_registry_binding` record identifying the canonical media directory. On Unix, the identity additionally contains the directory's filesystem device and inode numbers. `MediaRegistry::require_media_root_binding` reads that record without creating it and rejects missing, unbound or mismatched identity. `MediaRegistry::bind_media_root` uses a SQLite IMMEDIATE transaction and rejects attempts to change an existing binding. These methods do not authorize deletion by themselves.

## Operator workflow

For an existing unbound registry, first ensure it is the correct database and that the media directory contains no unexplained unregistered published files. Binding requires a bounded, full SHA-256/size/attribution/ownership integrity audit of **all registered objects** and an orphan scan reporting zero unregistered published objects. If the audit exceeds the configured bounds, fails integrity, or finds an orphan, binding fails without modifying registry identity. Such an installation requires a deliberate offline orphan reconciliation/migration procedure before binding; the command does not delete unexplained files to make its own preconditions pass.

```sh
cargo run -p wiki-pack -- media-maintenance --bind-registry /private/media /private/owners.sqlite 1000 1000
cargo run -p wiki-pack -- media-maintenance --dry-run /private/media /private/owners.sqlite 1000 1000
cargo run -p wiki-pack -- media-maintenance --confirm-delete /private/media /private/owners.sqlite 1000 1000
```

Both `--dry-run` and `--confirm-delete` now require the persisted binding to match the supplied root. The existing safety checks still require an initialized SQLite registry outside the media directory, a safe non-symlinked root and explicit destructive confirmation. The same-root bind is idempotent; attempting to bind another directory is rejected even if its contents are identical. No automatic rebind/move is provided.

## Verification

Source commit `8653cb09a1af54fa2498ef43bb40d879d73e0a0d` failed the [Rust formatting gate 40522252214](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40522252214), while [Platform shells 40522252178](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40522252178) and [Android Rust ABI 40522252163](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40522252163) passed. Corrected source SHA **`d5fe76991228476a31069c7fbbe505d9e66b7e42`** passed [Rust workspace 40522361834](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40522361834); [Platform shells 40522361688](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40522361688) was still running at documentation.

The regression suite checks SQLite reopen persistence, same-root idempotence, mismatched-root refusal, unbound maintenance rejection, orphan rejection during explicit binding, and existing pinned-media/unowned/orphan cleanup after successful binding.

## Limits and remaining tasks

This prevents **operator path mistakes** with cooperative local storage, not a malicious writer that can edit the SQLite binding or replace directories concurrently. Unix inode/device identity does not survive deliberate filesystem migration; migrations must be designed and audited separately. A clean full-object scan is required before first binding; large stores exceeding the bounded scan budget cannot bind via this command. It is not a publisher signature, pack authenticity proof, background maintenance scheduler, or Android device acceptance.

Outstanding STORE-002 work includes automatic lifecycle scheduling, hostile directory-race/uncooperative-writer hardening, disk-full and physical power-loss fault injection, upstream media/license authentication, and end-to-end pack/device acceptance. **Do not check STORE-002.**
