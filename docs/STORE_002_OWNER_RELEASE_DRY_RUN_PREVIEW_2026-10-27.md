# STORE-002 — Root-bound owner-release dry-run preview

Date: 2026-10-27  
Status: **partial qualification; STORE-002 remains unchecked**

## Behavior

`wiki-pack media-owner-release --dry-run <media-root> <existing-registry.sqlite> <pack|cache|pin> <owner-id> <max-candidates>` is an explicit read-only counterpart to the existing `--confirm-release` operation. Both require a safe existing media root, an initialized SQLite ownership registry outside that root, an exact valid owner kind/ID and a 1–10,000 registered-GC candidate budget. Unbound or wrong-root registries are refused.

The new `wiki_store::media_gc::preview_bound_owner_release` takes one SHARED media lease **before** checking the persistent root binding, dangling references, the requested owner's bounded claims and each candidate's owner count. It verifies complete SHA-256 and registered byte sizes for both would-be last-owner objects and previously unowned registered objects. The result distinguishes (a) how many claims would be revoked, (b) which registered objects would newly become unowned, (c) already-unowned objects independently eligible for the same GC pass, and (d) whether the combined count exceeds the collector's bounded budget. The CLI prints an explicit warning if a confirmed release could commit but the bounded GC sweep could fail with too many candidates.

The preview **does not mutate ownership, delete published media, or confer deletion authority**. It is advisory: SQLite writers that do not cooperate with the media lease can change ownership, and any later confirmation performs a fresh EXCLUSIVE-lease/SQLite-IMMEDIATE check. Lease setup may create its private coordination SQLite file, but no published media or owner rows are changed.

## Regression evidence

- Preview refuses unbound registries and accepts the same valid bound root used by the destructive command.
- Two pack owners and a user pin share a digest; the preview correctly identifies the requested pack's private digest as newly unowned, retains the shared digest, and leaves the owner's registered claims unchanged.
- An existing unowned object plus two would-be unowned pack objects exceeds a two-candidate GC budget. The preview warns but does not mutate data. Confirming anyway reports the already-committed owner release separately from the cleanup failure; a later explicitly confirmed maintenance pass with a sufficient budget safely recovers all unowned objects and retains pinned media.

Initial source `638ffdd658a143d59e84b5abdfeb49bb47c40037` failed rustfmt ([Rust 40561194359](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40561194359)). Corrected preview implementation `6d89055973d95f5dbaddad68e8d5dbd63109a0ea` passed [Rust 40561369811](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40561369811) and [Platform shells 40561369892](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40561369892). The new over-budget test first failed rustfmt at `a7a10fd5fbb8ba555d44cbf025ee2023e414b271` ([Rust 40561745860](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40561745860)), then a missing mutable fixture binding at `3c52d47e4792d76bdf2414482612ae9a2a092bde` ([Rust 40561885378](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40561885378)). Corrected test source `a376ff2b64fc7d3b0a63d80dd0a797e5026672e0` passed [Rust 40562134118](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40562134118); [Platform shells 40562134313](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40562134313) was running at documentation.

## Remaining work

This operator diagnostic is not an installed-pack lifecycle transaction or a user-facing uninstall. Remaining STORE-002 work includes durable catalog/snapshot coordination, integration with actual application pack/cache/pin lifecycle and opted-in host scheduling, hardware/device acceptance, real disk-full/power-loss testing, protection from hostile same-UID/unleased writers, and publisher-authenticated provenance. Do not mark STORE-002 complete.
