# STORE-002: Read-only registered-media GC preflight (2026-10-26)

**Status:** Partial GC planning; STORE-002 remains unchecked.

A new `media_gc::plan_unowned_media_gc` API queries only currently unowned SQLite-registered digests with a caller-specified bound (1–10,000 candidates), streaming-verifies each content-addressed object, checks its registered size, and returns deterministic candidate identities and potential reclaimed bytes. Invalid bounds, an over-limit candidate set, missing files, corruption and mismatched sizes abort the entire plan without modifying either SQLite or the filesystem.

This is an **advisory dry-run**, not a deletion API. It cannot discover bytes staged before an interrupted ownership transaction, and cannot prove a digest will remain unowned after the SQLite read. A production collector must coordinate object writers/readers and ownership transactions, discover unregistered orphan bytes, and use a crash-safe protocol before unlinking anything. This scoped feature does not authenticate manifests, activate packs, or qualify offline device behavior.

Synthetic Rust tests cover shared pack/cache/user-pin ownership, owner removal, deterministic candidate reports, limit rejection, corruption and missing files. Exact-head CI evidence will be recorded after validation.
