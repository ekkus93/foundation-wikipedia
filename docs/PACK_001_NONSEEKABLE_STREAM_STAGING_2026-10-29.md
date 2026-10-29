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
