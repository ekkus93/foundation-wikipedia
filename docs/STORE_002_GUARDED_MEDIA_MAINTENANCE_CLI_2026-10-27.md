# STORE-002 — Guarded local media maintenance CLI

Date: 2026-10-27  
Status: **partial qualification, CI queued at documentation; STORE-002 unchecked**

## Motivation

The core already supports lease-coordinated registered-unowned media reclamation and recovery of unregistered published orphans, but previously had no operator-accessible maintenance command. A naive command that calls `MediaRegistry::open` on a misspelled path silently creates an empty SQLite database, causing every published file to look unregistered and eligible for deletion. The new maintenance entry point rejects this failure mode.

## New functionality

`MediaRegistry::open_existing` uses a no-create SQLite open and requires all three expected registry tables. It rejects missing paths and uninitialized databases, preserving the ordinary `open` API for legitimate first-time initialization.

`wiki-pack media-maintenance` requires an explicit mode, existing media root, existing registry path **outside** the media tree, and two bounded limits:

```sh
cargo run -p wiki-pack -- media-maintenance --dry-run /private/media /private/owners.sqlite 1000 1000
cargo run -p wiki-pack -- media-maintenance --confirm-delete /private/media /private/owners.sqlite 1000 1000
```

`--dry-run` performs leased SHA-256-verified advisory plans, reporting object and byte counts without deletion. `--confirm-delete` invokes `run_bounded_media_maintenance` under its existing exclusive lease, returning a nonzero error if either the registered phase or orphan phase fails. Both limits must be in 1–10,000. Root lexical traversal, symlinked ancestors and a registry path inside the media root are rejected. A wrong or missing database is not initialized.

Regression tests cover missing/empty registry rejection, read-only dry-run, invalid confirmation and limits, pinned-media preservation, unowned registered and orphan reclamation, and repeated idempotent execution.

## Verification status

- Registry guard commit: `e616057a6ef471e28dd3e08154d4d49500e404b9`.
- CLI source commit: `c531db5f864dfda5a7f5942104c6631db26cd550`.
- [Rust workspace CI 40517080313](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40517080313): **queued at documentation**.
- [Platform shells CI 40517080094](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40517080094): **queued at documentation**.

Do not claim a CI pass until the exact source commit completes all relevant checks.

## Remaining safety and acceptance gaps

This CLI is intentionally **manual**, not a scheduled GC daemon. The registry schema check verifies the presence of expected table names, not a cryptographic binding of a particular registry to a particular media root; an operator must supply the correct matching registry. The underlying media lease excludes only cooperating processes. Hostile directory races, direct unleased SQLite writers, real power-loss/disk-full qualification, publisher-authenticated media inventory, pack activation and Android device acceptance remain outstanding.

**STORE-002 remains unchecked.**
