# STORE-002 — Atomic staged media ownership qualification

Date: 2026-10-10
Canonical checklist: `docs/WIKIPEDIA_AI_READER_TODO.md`
Status: **Partial implementation; STORE-002 and PACK-003 remain unchecked.**

## Implementation

- `crates/wiki-store/src/media_install.rs`: `install_owned_media_batch` validates all entries and owner metadata before any object writes, then stages every content-addressed object before registering any ownership.
- `crates/wiki-store/src/media_ownership.rs`: `register_verified_owned_batch` inserts verified content identities, all attribution notices and pack/cache/user-pin ownership in one SQLite transaction. A later conflict rolls back earlier rows, avoiding partially owned packs.
- Tests include a durable SQLite close/reopen and exact byte verification, multiple creator notices for shared bytes, pin retention after pack ownership is removed, invalid-input preflight and late SQLite size-conflict rollback.
- Failed object staging may leave **unowned** verified bytes; no installed pack is activated by these helpers.

## Exact-head qualification

- Initial implementation `233730e367d600b5c4f29bde672fa4eb2634934d` failed Rust formatting; corrected code `b6379f7ac385c9ce4dabdca8feb56e0d4e5ac23b` passed Rust workspace CI [38099327951](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38099327951), Platform shells CI [38099327845](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38099327845), and Android Rust ABI CI [38099327875](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38099327875).
- Transaction rollback test `be644e673ca4f33d026cd8213174ffc3f730cd5b` passed Rust workspace CI [38099736440](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38099736440), Platform shells CI [38099736435](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38099736435), and Android Rust ABI CI [38099736452](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38099736452).
- Android record codec probe [38099736422](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38099736422) was still running at the time of writing.

## Remaining work

This does **not** authenticate a manifest, prove every rendered visual is present, guarantee crash-safe garbage collection, atomically activate a complete pack, or qualify a real Android device in airplane mode. The authoritative TODO must not mark STORE-002 or PACK-003 complete. Its large existing body could not be updated through the available Ralph Bridge full-file write operation during this run; this document preserves the exact implementation and CI evidence for subsequent checklist reconciliation.
