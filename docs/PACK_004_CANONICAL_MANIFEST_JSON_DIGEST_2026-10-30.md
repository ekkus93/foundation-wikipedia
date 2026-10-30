# PACK-004 — Canonical manifest digest in JSON receipts (partial)

**Date:** 2026-10-30 (America/Los_Angeles)  
**Initial source SHA:** `0dd625075ed3004bcccbd0fd4b5ac2633c2194e5` (rustfmt failed)  
**Corrected source SHA:** `4430dad13365a0425bbd316d8a39c3fbe700e223`  
**Corrected-source CI:** [Rust workspace 41118165524](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41118165524) and [Platform shells 41118165518](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41118165518) — both **passed**.  
**Documentation CI:** [Rust workspace 41118455433](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41118455433) and [Platform shells 41118455774](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41118455774) — both **passed**.

## Scope

Successful `wiki-pack verify`, `inspect`, `verify-stdin`, `inspect-stdin`, `stage-unsigned`, `stage-unsigned-stdin` and `export-custom` JSON receipts now contain `manifest_sha256`: the lowercase SHA-256 of `Manifest::identity_transcript()`. This canonical transcript binds the format version, pack ID, project, snapshot, claimed origin, object paths, object digests and declared lengths. It is **not** a hash of the complete archive, a publisher signature, or proof of immutable storage after the command exits.

The file/stdin verify and inspect JSON modes now also include `schema_version: 1` and `status: "untrusted_verification"`, aligning their response contract with existing staging and export receipts. Existing human-readable output and byte-budget syntax remain compatible. Successful verification/staging/export still reports `publisher_signature_verified: false`, `offline_media_complete: false`, and `installed: false`. An unsigned manifest claiming an official publisher remains unauthenticated.

Example new fields:

```json
{
  "schema_version": 1,
  "status": "untrusted_verification",
  "manifest_sha256": "<64 lowercase hex characters>",
  "integrity_verified": true,
  "publisher_signature_verified": false,
  "offline_media_complete": false,
  "installed": false
}
```

## Regression tests

`tools/wiki-pack/tests/verify_budget_cli.rs` independently hashes the raw transcript from the archive and checks file/stdin parity, inspect/verify parity, and differing digests when the claimed origin changes despite identical object bytes. `stage_json_cli.rs` independently hashes the fixture and checks file/stdin staging receipts. `export_json_cli.rs` independently hashes the exported envelope transcript and compares its receipt with a subsequent `inspect --json` result. The initial source failed Rust formatting ([Rust workspace 41117973606](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41117973606)); the corrected source applies the exact formatting changes from CI. Both corrected-source and subsequent documentation exact-head CI workflows passed.

## Outstanding acceptance

The digest is only an unauthenticated identity receipt. A malicious producer can forge a valid unsigned archive and its digest. PACK-004 still needs production source-to-pack orchestration, mandatory offline visuals, installation/activation, and desktop/Android offline qualification; TRUST-001/002/003 need publisher signing/trust and hostile-input acceptance. None of those tasks is marked complete.
