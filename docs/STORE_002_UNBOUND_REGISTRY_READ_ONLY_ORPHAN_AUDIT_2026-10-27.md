# STORE-002 — Read-only audit for unbound legacy media registries

Date: 2026-10-27  
Status: **Partial qualification; STORE-002 remains unchecked.**

## Motivation

The guarded maintenance CLI refuses `--dry-run` and `--confirm-delete` when the existing SQLite ownership registry has not been explicitly bound to the media directory. The `--bind-registry` path correctly refuses any published digest missing from that registry, but previously the operator could not enumerate those orphans through the same guarded CLI. This left safe reconciliation of legacy registries difficult without granting inappropriate deletion authority.

## Implemented read-only diagnostic

`wiki-pack media-maintenance --audit-unbound <media-root> <existing-registry.sqlite> <max-registered> <max-scanned>` opens only an **existing initialized** SQLite registry outside the media root and uses the existing root-path/symlink safety checks. It calls `media_gc::audit_media_for_registry_binding`, which acquires one SHARED cross-process media lease, audits the complete bounded registered inventory (SHA-256, sizes, attribution and owners), then audits the complete bounded published-object inventory for unregistered digests, verifying their actual bytes. It returns a typed `MediaBindingPreflight` with a `RegisteredMediaAudit` and a `MediaGcPlan`. The CLI prints every orphan digest and byte size and warns that the operator must reconcile provenance before binding.

No registry binding is created, no owner rows are changed, and no file is deleted. Invalid budgets and corrupt registered or orphan bytes fail closed. The existing `--bind-registry` API reuses the same internal preflight helper but retains its EXCLUSIVE lease from start to final binding commit and still refuses unexplained orphans.

## Tests and CI

The Rust test creates a registered/pinned object and an unregistered published object, checks the reported exact SHA-256 digest and byte totals, verifies an unbound registry stays unbound, and proves both files remain readable. CLI regression checks that `--audit-unbound` succeeds for this state while ordinary `--dry-run`, `--bind-registry` and `--confirm-delete` remain prohibited. Invalid limits are rejected.

Initial source SHA `9df0ff0c6b221225504e128ab6455beb80c74a00` failed rustfmt ([Rust 40536338352](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40536338352)). Corrected source SHA `99d3eaddcd8a3864b593666ad601af5bfc256c46` passed [Rust workspace 40536471066](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40536471066), [Platform shells 40536470995](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40536470995), and [Android Rust ABI 40536471205](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40536471205). Android record-codec probe 40536471655 was still running when recorded.

## Scope limits and next work

The command is **diagnostic only**. It does not prove whether an unregistered file belongs to another registry, grant deletion authority, synthesize attribution or adopt unknown bytes. An operator must investigate before moving, registering or deleting any orphan. Coordinating direct unleased SQLite writers, hostile filesystem races, automated maintenance scheduling, physical power-loss qualification, full authenticated pack lifecycle and Android/desktop acceptance remain outstanding.
