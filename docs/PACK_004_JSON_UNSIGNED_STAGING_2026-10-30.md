# PACK-004 — Machine-readable unsigned pack staging (partial)

**Date:** 2026-10-30 (America/Los_Angeles)  
**Implementation:** `8f992fc64a27f7c43d0966e89d32fcb93fcd8ec7`  
**Black-box tests:** `824694d3012b391e2870b16ab1843067dc09070f`  
**Qualified formatting correction:** `a0668d614fb889e3f95a41fb1605f53808b4efa4`

## Scope and integration contract

The provisional `wiki-pack` CLI now offers opt-in `--json` for both staging transports, matching the existing machine-readable read-only inspection mode:

```sh
wiki-pack stage-unsigned --json input.wpack /private/pack-staging candidate 104857600
wiki-pack stage-unsigned-stdin --json /private/pack-staging candidate 104857600 < input.wpack
```

`--json` must precede positional arguments. File staging retains its optional maximum input-byte budget; stdin staging requires a byte quota. The parent must be a caller-owned private directory. Existing human-readable command syntax and warnings are unchanged.

A successful invocation emits exactly one JSON object **after** the complete envelope has been integrity-verified, its objects copied and rechecked, and the staging tree durably published. The schema is version 1:

```json
{
  "schema_version": 1,
  "status": "untrusted_staging",
  "staging_path": "/private/pack-staging/candidate",
  "integrity_verified": true,
  "publisher_signature_verified": false,
  "offline_media_complete": false,
  "installed": false,
  "pack_id": "science",
  "project": "enwiki",
  "snapshot": "20261030",
  "object_count": 1,
  "declared_payload_bytes": 7,
  "claimed_origin": "official",
  "claimed_origin_identity": "unverified-publisher"
}
```

`staging_path` is the resulting filesystem path, not an activation pointer or a trust anchor. `claimed_origin` and its identity come from **untrusted** manifest data. A claimed official origin does not authenticate the publisher. Callers must not treat the success receipt as permission to install, activate, index or expose the pack to a reader. The schema intentionally has no verified-publisher key identity.

Malformed invocations exit 2; failed staging exits 1, prints diagnostics on stderr and emits **no successful JSON object**. On verification, quota, collision or extraction failures, the staging library does not publish a usable destination. An unseekable input is spooled privately within the supplied quota and cleaned up.

## Regression and CI

`tools/wiki-pack/tests/stage_json_cli.rs` invokes the real CLI with a synthetic unsigned envelope **claiming an official publisher**. It verifies file/stdin JSON parity, complete staged object bytes, conservative negative trust flags, retained human-readable warnings, exact-budget success, one-byte-short rejection, occupied-destination rejection, missing stage outputs and temporary spool cleanup, and usage errors.

The first implementation and test commits were followed by a rustfmt correction after [Rust CI 41092630897](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41092630897) identified formatting differences. The corrected exact source passed [Rust workspace CI 41092770685](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41092770685) (format, Clippy, workspace tests) and [Platform shells CI 41092770107](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41092770107).

## Remaining acceptance

This is a provisional **unsigned staging** interface. Publisher signature verification, signed official catalog trust, required offline image/diagram/math/caption/reference/attribution completeness, production pack installation/activation, desktop/Android integration and real-device acceptance remain outstanding. `PACK-004`, `TRUST-002` and `TRUST-003` are not complete.

## Follow-up: exact UTF-8 staging paths

The initial JSON receipt used lossy path formatting, which could silently misidentify a staged directory whose parent contains non-UTF8 filesystem bytes. Follow-up code **`d9df9135a46d06cea72dcbc1947f9dd768a94196`** rejects non-UTF8 JSON destinations **before** opening the file input or consuming the stdin stream, and then serializes the exact validated UTF-8 path. Human-readable staging is unchanged. The Unix black-box regression exercises both transports with a real non-UTF8 parent and checks exit 1, no success JSON and no staging residue. [Rust workspace CI 41093702752](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41093702752) passed; [Platform shells CI 41093702749](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41093702749) was still running at this documentation update.
