# STORE-002 / STORE-003 — Root-bound pack media activation gate (2026-10-27)

## Scope

Qualified source commit: `abd47f627a259c3891ea6b7d8f7c2af9fe3ca135`.

`crates/wiki-store/src/activation.rs` now exposes `ActiveSnapshotStore::switch_to_prevalidated_with_bound_pack_media(id, store, registry, required)`. This is a **new opt-in API**, not a replacement for the low-level `switch_to_prevalidated` primitive and not yet wired to desktop/Android production pack installation.

Before the pointer can switch, the method requires an ID that matches the snapshot selector's safe-ID contract; holds a shared cross-process media lease; verifies the registry is bound to the supplied media root; and compares the exact registry claims of `OwnerKind::Pack` for owner `id` to the caller-supplied required digest/size manifest. Every claimed object is read/hashed under that lease. If any prerequisite fails, the current snapshot pointer is unchanged. Only after passing these checks does it invoke the existing durable prevalidated pointer switch, still under the **same media lease**, then return verified byte count.

The snapshot selector and pack owner share an exact ID. This prevents an unrelated owner from satisfying the media gate for another snapshot. The caller MUST independently verify and authenticate the manifest, all required visuals, the staged article/catalog/shard data, and the snapshot's provenance before invocation.

## Verified regression behavior

Disk-backed tests in `activation::interruption_tests` cover:

- An unbound registry and a registry bound to a different media root both fail without switching away from the old snapshot.
- A bound registry with missing pack claims, incorrect declared byte size, or corrupted object bytes fails while preserving the old pointer.
- A valid exact pack-media inventory activates the new snapshot, reporting its verified byte total; reusing that owner inventory for another snapshot ID fails.
- A competing raw SQLite `BEGIN EXCLUSIVE` on the media lease database is blocked after verification while the activation gate remains open, and succeeds after the method returns.

Exact-head CI on source SHA `abd47f627a259c3891ea6b7d8f7c2af9fe3ca135`:

- [Rust workspace run 40614642253](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40614642253) — passed format, strict Clippy, tests and workspace gates.
- [Platform shells run 40614642306](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40614642306) — passed.
- [Android Rust ABI run 40614642297](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40614642297) — passed.

Earlier source commits failed formatting and were corrected before qualification; these failures are not acceptance evidence.

## Limitations / next software tasks

The API relies on **caller-authenticated and complete** required-media inventories. It is not an authenticated manifest parser, pack installer, independent completeness oracle, or safe deletion protocol. A different caller could still invoke the lower-level pointer method without the media gate; platform adapters must use the verified entry point. This lease is advisory and only excludes cooperating garbage collectors; it does not protect against same-user direct writes, concurrent noncooperating snapshot switches, or subsequent owner revocation after activation. Retaining active and rollback snapshot owner claims requires explicit lifecycle integration and reliable recovery rules. Crash/power-loss/disk-full, exact offline rendering completeness, and real desktop/Android device qualification remain outstanding. Both STORE-002 and STORE-003 remain unchecked.
