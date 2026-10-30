# PACK-004 — Machine-readable provisional .wpack inspection (partial)

**Date:** 2026-10-30 (America/Los_Angeles)  
**Implementation:** `95e8adf23747628c40322d1281acd9a608a92aa3`  
**Formatting correction:** `c027302b72f3d55db72b709b49a4b549e6842e3f`  
**Expanded trust regression:** `abcf1efdc749427561c8aecc07204c540a0e7fcf`

## Problem and scope

The provisional `wiki-pack` CLI previously emitted human-readable integrity and trust disclaimers only. Automated desktop/Android pack-management workflows need a parseable inspection result without scraping text or confusing a self-declared official origin with a verified publisher signature.

Added opt-in `--json` to four **read-only** commands:

```sh
wiki-pack verify --json input.wpack
wiki-pack inspect --json input.wpack 104857600
wiki-pack verify-stdin --json 104857600 < input.wpack
wiki-pack inspect-stdin --json 104857600 < input.wpack
```

The optional file byte budget and mandatory stdin budget continue to include framing, transcript and payload bytes. Existing invocation forms without `--json` retain their human-readable output. The flag must precede the file argument or stdin budget. Invalid argument forms exit with usage status 2; failed integrity checks exit with status 1 and do not print a success JSON object.

## JSON contract

On successful local envelope verification, `verify --json` and `verify-stdin --json` emit one JSON object with:

- `integrity_verified: true`
- `publisher_signature_verified: false`
- `offline_media_complete: false`
- `installed: false`
- `object_count` and `declared_payload_bytes` (nonnegative integers)

`inspect --json` and `inspect-stdin --json` additionally expose `pack_id`, `project`, `snapshot`, `format_version`, `claimed_origin` (`custom` or `official`), and `claimed_origin_identity`. **Claimed origin is untrusted manifest data.** Neither the string `official` nor a publisher name implies signature verification, catalog authentication, source provenance, offline-media completeness or installation. JSON contains no signing-key assertion and cannot be used alone to authorize activation.

The output is deliberately a compact summary, not an object-by-object archive listing or a finalized signed-manifest schema.

## Regression and CI

The black-box test `tools/wiki-pack/tests/verify_budget_cli.rs` exercises the actual CLI binary for file and piped-stdin JSON output, both verification and detailed inspection, equality of results across transports, existing byte budgets, and preservation of explicit negative trust flags. A separate forged-official-origin test constructs a valid unsigned envelope with `Origin::Official` and verifies that JSON still reports `publisher_signature_verified: false`, `offline_media_complete: false`, and `installed: false`.

The initial implementation failed formatting only ([Rust workspace 41084003643](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41084003643)); the correction passed [Rust workspace 41084365639](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41084365639) and [Platform shells 41084366193](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41084366193). The expanded trust regression passed [Rust workspace 41084956078](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41084956078) and [Platform shells 41084956010](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41084956010).

## Remaining acceptance

`PACK-004` remains incomplete: authoritative source-to-selection/media/index/build orchestration, production export/install, official signature verification, measured storage estimates, mandatory offline visuals and Android device qualification are outstanding. `TRUST-002` also remains incomplete; callers must not equate this local inspection result with a trusted publisher pack.
