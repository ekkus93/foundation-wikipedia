# STORE-002 — Exclusive-lease registry binding and contention qualification

Date: 2026-10-27  
Status: Partial qualification; **STORE-002 remains unchecked**.

## Problem and implementation

Previously `wiki-pack media-maintenance --bind-registry` acquired and released a SHARED lease for the registered-media audit, then another SHARED lease for the orphan audit, then called `MediaRegistry::bind_media_root` without a media lease. A cooperating installer could publish new unregistered bytes between those checks or during the final SQLite binding, violating the precondition that a registry is bound only to a fully explained media tree.

`media_gc::bind_registry_after_complete_media_audit` now validates both bounded limits (1–10,000), acquires **one EXCLUSIVE media lease before inspecting any registry rows or filesystem objects**, verifies the complete registered inventory with notices, owners, declared sizes and streaming SHA-256, checks the entire bounded published-object inventory for unregistered orphans, and commits the root binding **before releasing the lease**. The existing audit and orphan planner expose internal already-leased helpers so no nested SQLite-backed lease is acquired. The CLI now uses this single API. No bytes are deleted by the binder.

The API fails closed for malformed/oversized registered inventories, corrupted objects, invalid limits and any unregistered published file. A clean repeat bind is idempotent. This remains an **operator-approved** binding of an existing registry, not a mechanism to adopt unexplained orphan files.

## Regression evidence

Tests exercise orphan preservation and no binding on failure, invalid budgets, corruption, successful full audit, repeated binding, and a separate SQLite registry connection with an active SHARED media lease. The binder is launched in another thread and must wait while that lease is held; the registry remains unbound until the reader releases it. This is a cooperating-lock contention proof, not hostile-writer protection.

Initial implementation `8cb792482809d92c41256e494d2d6a56eb8c9c03` failed rustfmt ([Rust 40534524865](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40534524865)); formatting follow-up `732d1e1cfe90b2ce4ceea8c7660f5d15808de16c` failed an enum comma formatting gate ([Rust 40534959336](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40534959336)). Corrected implementation `3e5887702cf13d173dae5f7d13fd149e5fd5622c` passed [Rust workspace 40535067830](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40535067830), [Platform shells 40535068235](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40535068235), and [Android Rust ABI 40535067876](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40535067876).

The contention regression `78e9ff75c44f2eb69a9f7fe9cf50000306cd209c` initially failed rustfmt ([Rust 40535559578](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40535559578)). Corrected exact source `c79b18d3b3148856e08d7926b5c57ea4bfd6040d` passed [Rust workspace 40535667501](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40535667501), [Platform shells 40535667611](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40535667611), and [Android Rust ABI 40535667520](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40535667520). Android record-codec probe 40535667431 was still running at documentation.

## Remaining boundaries

- The lease coordinates participating readers/installers/collectors; an unrelated direct SQLite writer or hostile filesystem writer can bypass it. No claim of atomic protection against adversarial directory replacement is made.
- Existing unbound registries with orphaned media still need a safe, operator-reviewed reconciliation path. Do not silently bind or delete unregistered bytes.
- Automatic maintenance scheduling, process-kill and power-loss qualification on target storage, authenticated upstream media provenance, complete pack lifecycle integration and desktop/Android device acceptance remain open.

Do not mark STORE-002 complete based on these scoped tests.
