# PACK-004 — Machine-readable custom pack export (partial)

**Date:** 2026-10-30 (America/Los_Angeles)  
**Implementation and black-box tests:** `bfed64d63c1638c1975f670b9c5ab499729b21b2`  
**CI at documentation:** [Rust workspace 41101404089](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41101404089) and [Platform shells 41101403978](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41101403978), both queued; do not infer qualification until exact-head success.

## Scope

`wiki-pack export-custom [--json] <object-dir> <pack-id> <project> <snapshot> <definition-id> <output.wpack>` now supports opt-in machine-readable output, complementing the existing `verify`, `inspect`, and unsigned staging JSON modes. Existing human-readable export syntax and warnings are preserved.

On successful export the CLI inventories and verifies the immutable private object tree, writes and synchronizes a temporary envelope, reopens and re-verifies the complete archive, publishes without overwriting an existing output, removes the temporary file, and (on Unix) synchronizes the destination directory. **Only after those steps** does it print one JSON receipt. The existing rollback behavior for a failed directory sync remains in force.

Example successful response (representative object sizes):

```json
{
  "schema_version": 1,
  "status": "untrusted_export",
  "output_path": "/private/exports/science.wpack",
  "integrity_verified": true,
  "publisher_signature_verified": false,
  "offline_media_complete": false,
  "installed": false,
  "pack_id": "science",
  "project": "enwiki",
  "snapshot": "20261030",
  "object_count": 2,
  "declared_payload_bytes": 13,
  "claimed_origin": "custom",
  "claimed_origin_identity": "user-definition"
}
```

The output path must be representable exactly as UTF-8 in JSON mode. A non-UTF8 destination is rejected **before reading the source object tree or publishing any output**, rather than silently substituting replacement characters. Human-readable export remains available for filesystem paths that cannot be encoded in JSON. Malformed invocations exit 2; export failures exit 1 with stderr diagnostics and no success JSON.

## Regression coverage

`tools/wiki-pack/tests/export_json_cli.rs` exercises the actual executable with a two-object source tree. It checks the receipt and conservative trust flags, validates the resulting archive independently with `inspect --json`, verifies collision rejection without modifying the existing file or leaving a temporary file, confirms legacy human-readable output, rejects missing arguments without a success receipt, and on Unix rejects a non-UTF8 output name without creating a file.

## Acceptance still outstanding

The archive remains **unsigned and untrusted**. Neither the receipt nor its SHA-256 object verification proves publisher identity, required offline diagrams/images/math/captions/attribution completeness, a production installer, pack activation, or physical Android/desktop acceptance. `PACK-004`, `TRUST-001`, `TRUST-002`, and `TRUST-003` remain unchecked. Exact-head CI must be reconciled before treating this code slice as qualified.
