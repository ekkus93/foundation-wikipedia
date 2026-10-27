# STORE-002 — SQLite media-registry pathname isolation

**Date:** 2026-10-26  
**Scope:** Partial STORE-002 security prerequisite for durable ownership and future garbage collection; no byte deletion or task closure.

## Problem

The media ownership registry previously opened a caller-supplied SQLite path directly. An existing symlink, multiply hardlinked database, nonregular file, or symlinked ancestor could alias the mutable ownership and attribution database outside the intended storage location. SQLite could also create a new registry with permissions determined by a permissive process umask.

## Implementation

- Before opening a disk-backed registry, reject parent-directory traversal components and validate every ancestor of the registry's parent as a real directory rather than a symlink.
- Reject symlinked and nonregular database paths, and on Unix reject existing SQLite database inodes with more than one hard link.
- When the database is absent, reserve it with exclusive create-new semantics; on Unix use mode 0600. A competing creation after preflight fails closed rather than opening an unexamined file.
- Keep the in-memory registry API unchanged. Preserve ordinary reopen of a single-linked existing registry and its registered object metadata.
- Return a distinct `UnsafePath` error for unsafe aliases; filesystem failures are returned as `Io`.

This is a **path preflight**, not a secure directory-handle or SQLite VFS implementation. It cannot prevent an untrusted process with directory write access from renaming components after validation. Existing database file modes and SQLite journal/WAL sidecars are not retroactively hardened here.

## Regression and CI evidence

Unix test `disk_registry_rejects_symlink_hardlink_and_traversal_aliases` verifies private file creation, persisted attribution on reopen, rejection of a symlinked file, both names of a multiply hardlinked database, a symlinked parent, a parent-traversal path and a directory substituted for a database. It also verifies reopening succeeds after the extra hardlink is removed.

Exact implementation commit: `a7fb5e8e5f577b89652ba4a051a02b57bfc1b573`.

- [Rust workspace CI 40451856132](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40451856132) — passed (formatting, strict Clippy, workspace tests).
- [Android Rust ABI CI 40451856237](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40451856237) — passed.
- [Platform shells CI 40451856220](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40451856220) — in progress at documentation.
- [Android record codec probe 40451856118](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40451856118) — in progress at documentation.

## Remaining work

Production media garbage collection still needs a coordinated cross-process protocol for active installers/readers and the SQLite registry, safe atomic byte retirement, and crash/power-loss recovery. The registry pathname preflight does not authorize deleting any object and does not prove a complete publisher-authenticated offline pack, attribution authenticity, full required media coverage or physical-device behavior. **STORE-002 remains unchecked.**
