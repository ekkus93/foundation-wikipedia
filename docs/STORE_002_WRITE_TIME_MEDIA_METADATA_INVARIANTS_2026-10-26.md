# STORE-002: Transactional media metadata invariants (2026-10-26)

**Status:** Scoped implementation and regression qualification. STORE-002 remains unchecked.

## Defect and repair

The media registry's owner-scoped readers and integrity audits reject conflicting MIME declarations for identical content bytes and reject more than 1,024 attribution notices per digest. Previously, both public registration paths could **create** those invalid states during an otherwise legitimate install: a second pack could register the same digest with a different MIME, or a stream of distinct creators could exceed the reader's limit. Once that happened, an existing pack's visual could become unreadable even though its bytes and ownership were unchanged.

`MediaRegistry::register_verified_bytes` and `register_verified_owned_batch` now enforce the same one-MIME-per-digest invariant inside the SQLite transaction that inserts notices. A conflicting declaration fails with `ConflictingMime` and rolls back the entire batch, including preceding objects, notices and owner claims. Distinct creators, licenses and source URLs with the **same** MIME remain supported.

Both paths also enforce the 1,024-notice ceiling **before insertion**. An identical already-registered notice remains idempotent at exactly 1,024; a new distinct notice fails with `TooManyNotices`, and an already-over-limit corrupted database fails closed. A batch exceeding the ceiling rolls back in full. Single-object registration additionally rejects payloads larger than the object store's 64 MiB maximum before hashing or persisting anything.

## Tests and compatibility

Synthetic Rust regressions cover single-write MIME conflicts without mutation; shared pack ownership and readable raster assets after a rejected conflicting notice; same-MIME multiple creators; batch conflicts both within one batch and against existing owners; exact-at-limit idempotence, 1,025th-notice refusal, and atomic batch rollback; and oversized single writes. Existing reader/audit tests that intentionally inspect contradictory MIME now inject **historical SQLite corruption** directly instead of using a public registration API that correctly rejects the state. The read-side fail-closed protections remain independently tested.

These changes do not infer MIME from bytes or authenticate upstream attribution; they only enforce consistency of the declared metadata. Installers still need to verify the actual media type and complete required-visual inventory. SQLite transactions serialize registry mutations but do **not** coordinate filesystem GC with readers or installers.

## Exact-head evidence

- Corrected implementation SHA: `5908dc1d1e14eb43d00fb25596fdcf92746bc111`.
- Rust workspace (fmt, strict Clippy, tests): [40434451484](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40434451484) — passed.
- Android Rust ABI qualification: [40434451500](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40434451500) — passed.
- Platform shells: [40434451474](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40434451474) — still running when this document was written.
- Earlier attempts: `755362be` failed rustfmt; `2aa88263` exposed an old builder test that relied on accepting a conflicting MIME; `cdddc401` passed Rust workspace after that test was corrected; `56d9831b` failed strict Clippy's range-contains lint; `5908dc1d` corrected it.

**Still outstanding:** concurrency-safe GC deletion and crash recovery, full required offline visual discovery, authenticated publisher/source manifests, pack activation, and desktop/Android physical acceptance. No top-level STORE-002 completion is claimed.
