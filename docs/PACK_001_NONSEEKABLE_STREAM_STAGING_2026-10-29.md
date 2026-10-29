# PACK-001 / TRUST-003 — Nonseekable Pack Stream Staging (2026-10-29)

## Implementation

The provisional unsigned `.wpack` envelope can now be staged from an arbitrary `Read` source without `Seek`. Public `wiki_pack_format::stage::stage_container_stream` and `stage_container_stream_with_trust` target document-provider streams, including Android Storage Access Framework sources that cannot seek.

The caller provides a private, exclusively controlled staging parent and validated new directory name. Shared preflight rejects unsafe names, symlinked ancestors and non-private Unix parent permissions before reading the input. A private, uniquely named `create_new` spool receives only bytes consumed by `verify_container`, which checks the canonical transcript, resource limits, object SHA-256/lengths and trailing data. After spool fsync and rewind, existing `stage_container` independently re-verifies and materializes objects. The spool is deleted on success and failure. No pack is activated or publisher-authenticated.

## Evidence

- Source SHA: `b4deed33ad952978717fd7a49644fb4f3beb49e5`. Initial `697674924d10bb431ae7f3be127160bcffefe757` failed rustfmt and was corrected.
- Rust workspace (format, Clippy, tests): https://github.com/ekkus93/foundation-wikipedia/actions/runs/40868726201 — passed on exact source SHA.
- Platform shells: https://github.com/ekkus93/foundation-wikipedia/actions/runs/40868726039 — passed on exact source SHA.
- Android Rust ABI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/40868726102 — passed on exact source SHA.
- Synthetic tests: nonseekable input, correct object materialization, conservative custom trust, spool cleanup, digest corruption, appended data and invalid-name rejection before reading.

## Outstanding

This is bounded-memory local-integrity staging, **not** a signed publisher format, Android SAF integration, authenticated offline media/attribution completeness, atomic install/rollback or physical-device test. A writer sharing the private parent can race pathname checks; descriptor-relative race-resistant access and power-loss qualification remain pending. The spool requires disk space proportional to archive size; callers must enforce free-space and quota policies. PACK-001 and TRUST-003 remain unchecked.

## Follow-up: cleanup on spool-removal error and provider disconnect

A successful staging operation must not be reported as failed while leaving a newly materialized pack directory behind merely because temporary spool removal failed. The importer now removes the staged tree before returning that cleanup error. The additional synthetic test injects a document-provider `BrokenPipe` after a syntactically valid envelope header and checks that the private parent is empty after failure.

Final code SHA `f44eb31b02015556b76b00b59533042bc081fd9f` passed exact-commit [Rust workspace CI 40869777599](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40869777599), [Platform shells CI 40869777597](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40869777597), and [Android Rust ABI CI 40869777594](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40869777594). An intermediate bounded-file-read truncation was corrected before qualification; `9f2ba2b5895479be940b071b974fa959267315c5` and `34c6841f01674ffea3f1fc6cc95a1f26a56c4c51` failed Rust formatting/parsing and must not be used as release evidence. OS-level cleanup failure, concurrent directory mutation, storage exhaustion and power loss still require production hardening and device testing.
