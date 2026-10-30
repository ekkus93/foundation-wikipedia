# PACK-004 / TRUST-003 — Seekable staging input-byte budget (partial)

**Date:** 2026-10-30 (America/Los_Angeles)  
**Implementation:** `373906e90a8652ce2fc737c540b5a64a459a0c56`  
**Black-box regression:** `9d51b9be5fb43f0b9655d2bbe6aec80f7d4e2d21`  
**Scope:** Provisional unsigned `.wpack` staging, not installed-pack activation or publisher authentication.

## Problem

The nonseekable stdin staging path had an explicit spool-byte quota, but the seekable file staging path had no caller-selected input-byte limit. An otherwise well-formed envelope could require more verification I/O and staged disk than a caller intended, even though the manifest-level global limits remained in force.

## Implementation

- Added `wiki_pack_format::stage::stage_container_with_budget` for seekable `Read + Seek` sources. It retains destination preflight, two-pass canonical manifest transcript verification, object digest and complete inventory checks, and fail-closed cleanup.
- The input budget includes the fixed 12-byte header, canonical transcript and declared object payloads. A forged transcript longer than the quota is rejected after reading only the header; a complete manifest declaring more object bytes than the quota is rejected before reading the payload or creating a staging directory.
- Extended `wiki-pack stage-unsigned <file.wpack> <private-parent> <name> [max-input-bytes]` with an optional decimal quota. The existing three-argument command remains supported.
- Unit regression covers exact-byte success, one-byte-short rejection before object I/O, oversized forged transcript preflight, too-small header budget and absence of partial output. A real CLI integration test checks both command forms, invalid budgets and explicit untrusted staging labels.

## Qualification

- Exact implementation SHA `373906e90a8652ce2fc737c540b5a64a459a0c56` passed [Rust workspace](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41058936083) and [Platform shells](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41058935878).
- The black-box test head `9d51b9be5fb43f0b9655d2bbe6aec80f7d4e2d21` has [Rust workspace](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41059474870) and [Platform shells](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41059474912) CI; reconcile exact-head outcomes before treating this as fully qualified.
- Initial implementation SHA `c9b50582e37fd9bbd1ad6a69e2fb889369d10ff5` and initial black-box test SHA `2b253de6ea07e7c34d038da8b12e7691c74fbb51` failed rustfmt only; both were corrected.

## Remaining

This is an opt-in resource ceiling, not a filesystem free-space reservation. The legacy seekable API and CLI without the optional argument retain their existing unbounded caller-selected behavior (subject to manifest limits). Publisher signatures, required offline media and attribution, transactional installation, Android document-provider integration and physical-device/power-loss acceptance are still outstanding. **PACK-004 and TRUST-003 remain unchecked.**
