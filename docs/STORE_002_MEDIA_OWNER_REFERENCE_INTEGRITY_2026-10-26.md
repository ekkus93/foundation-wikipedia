# STORE-002 — Registered media ownership/reference audit (2026-10-26)

The read-only registered-media integrity audit now validates bounded pack/cache/pin ownership rows, detects dangling ownership/attribution rows, and rejects contradictory MIME attribution for identical bytes. Per-object owner fanout is capped at 1,024; unowned objects remain valid GC candidates.

SQLite tampering regressions exercise malformed owner IDs, dangling owner/notice rows, conflicting MIME, owner fanout, and recovery after repair. This does not delete bytes, establish publisher authenticity, prove media completeness, or qualify concurrency-safe garbage collection, pack activation, or physical-device acceptance. STORE-002 remains unchecked.

Qualification requires exact-head Rust workspace and Platform shells CI, plus relevant Android ABI evidence.
