# STORE-002 — Single-lease root-bound maintenance preview

Date: 2026-10-27  
Status: **partial qualification; STORE-002 remains unchecked**

## Change

The CLI's previous `media-maintenance --dry-run` path checked the bound registry root outside a lease, then acquired **two separate** SHARED leases for the registered-unowned and unregistered-orphan plans. A cooperating EXCLUSIVE collector could run between those two snapshots, so the combined report could mix different media-store states.

`media_gc::preview_bound_media_maintenance(store, registry, max_registered, max_scanned)` now validates both 1–10,000 budgets before accessing storage, acquires a **single SHARED cross-process media lease**, checks the persistent registry/media-root binding, audits dangling SQLite references, and streams full SHA-256/size verification of registered-unowned objects. It then scans and verifies unregistered published orphans without releasing the lease or recursively acquiring another SQLite lease. It returns `BoundMediaMaintenancePreview { registered, orphans }` and **does not modify** the registry or unlink bytes.

`wiki-pack media-maintenance --dry-run` now uses this combined preview and reports the same counts/bytes as before. Confirmed deletion still takes its own EXCLUSIVE lease and revalidates independently; preview is never deletion authority. This only coordinates **cooperating** lease users, not direct unleased SQLite writers or malicious filesystem replacement.

## Tests and exact-head CI

The synthetic regression creates a pinned registered object, an unowned registered object and an unregistered published orphan. It first rejects an unbound registry, binds the fixture, then verifies both plans and byte totals while preserving all files and the pin. It also checks invalid budgets fail and that corrupt orphan bytes abort the preview without mutating the registered inventory.

Initial source `03bef2fa7e75d25c505f6a1222571155bad800f0` failed rustfmt ([Rust 40571660040](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40571660040)). Corrected source **`f5f6cf84ba01733bb90d72cc625e1d1a178c03be`** passed [Rust workspace CI 40571829689](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40571829689), [Platform shells CI 40571829641](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40571829641), and [Android Rust ABI CI 40571829664](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40571829664). [Android record codec probe 40571829661](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40571829661) was still running when recorded.

## Remaining

Actual pack lifecycle integration, physical power-loss/disk-full testing, hostile same-UID/unleased writer protections, publisher-authenticated required-media provenance, Android/desktop device acceptance and timer activation remain outstanding. The combined preview is a safer **read-only** observation, not a guarantee of an immutable SQLite ownership snapshot.

**STORE-002 remains unchecked.**
