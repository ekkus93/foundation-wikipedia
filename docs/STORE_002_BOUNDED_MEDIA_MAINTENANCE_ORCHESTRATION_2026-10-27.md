# STORE-002 — Bounded registered/orphan media maintenance orchestration

Date: 2026-10-27  
Status: **partial qualification; STORE-002 remains unchecked**

## Implementation

`media_gc::run_bounded_media_maintenance(store, registry, max_registered_candidates, max_scanned_objects)` is a caller-triggered maintenance API. It validates both bounded budgets (1–10,000) **before** mutation, takes one EXCLUSIVE cross-process media lease, and runs registered-unowned SQLite-first reclamation followed by published-but-unregistered orphan recovery. The existing collectors were factored into private lease-held helpers to avoid recursively acquiring the SQLite-backed lease while an exclusive lease is held.

The returned `MediaMaintenanceOutcome { registered, orphans }` exposes independent results for the two phases. An outer `Err` indicates preflight/lease failure before either phase. A successful outer result does **not** mean both phases passed; the caller must inspect both. The registered phase may commit SQLite deletion before a filesystem unlink fails; the subsequent orphan phase can safely reclaim the now-unregistered bytes under the same lease. Other packs, cache owners and user pins retain their references and bytes. Both phases remain bounded, fail closed on malformed/unsafe content, and use streaming SHA-256 verification.

## Regression coverage and CI

- Normal combined pass reclaims two registered-unowned objects and one unregistered orphan, preserves user-pinned bytes and attribution, and is idempotent on a repeated pass.
- Invalid registered and orphan scan budgets reject the entire call without releasing registered ownership or deleting files.
- A deterministic injected post-commit registered unlink error leaves a published unregistered object; the orphan phase recovers it alongside an earlier orphan, reports exact reclaimed bytes, and preserves a user pin.

Initial source SHA `8650c30f02654d3f6e03337612edb79d0ea606cf` failed formatting ([Rust workspace 40514147223](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40514147223)). Corrected source SHA **`6a541611cde0f5186b8ba9e745ae5a212ed3abc4`** passed [Rust workspace CI 40514392447](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40514392447) and [Android Rust ABI CI 40514392608](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40514392608). [Platform shells 40514392469](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40514392469) and [Android record codec probe 40514392342](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40514392342) remained in progress when recorded.

## Explicit limitations

This is a callable maintenance primitive, **not** a scheduler or installed background job. It is not yet integrated into actual pack/cache/pin deletion workflows or a desktop/Android maintenance UI. It does not guarantee liveness for inventories exceeding a single bounded scan or resolve corrupted object inputs automatically. A cooperative shared/exclusive lease is not protection against hostile direct SQLite or filesystem writers, and deterministic fault injection is not proof against real power-loss or disk-full failures. Publisher authentication, required-visual completeness, atomic pack activation and device acceptance remain outstanding.

**Do not mark STORE-002 complete.**
