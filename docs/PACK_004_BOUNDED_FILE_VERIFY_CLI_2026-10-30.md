# PACK-004 — Optional byte-bounded file verification (partial)

**Date:** 2026-10-30 (America/Los_Angeles)  
**Corrected implementation:** `febc48edbdefba81d22638afaab03f7a4d4b943a`  
**Scope:** Read-only verification and inspection of provisional unsigned `.wpack` files.

## Problem

The CLI already supported `verify-stdin <max-input-bytes>`, `inspect-stdin <max-input-bytes>`, and an optional byte budget on `stage-unsigned`. The ordinary seekable-file `verify` and `inspect` commands offered no caller-selected resource ceiling, leaving operators unable to cap a potentially large untrusted envelope before verification.

## Implementation

Both commands now accept an optional decimal `max-input-bytes` argument:

```sh
wiki-pack verify input.wpack 104857600
wiki-pack inspect input.wpack 104857600
```

The existing `wiki-pack verify input.wpack` and `wiki-pack inspect input.wpack` forms remain supported. When a ceiling is supplied, the CLI uses the shared `verify_container_with_budget` API. It rejects a transcript declaration exceeding the limit before allocating the transcript, rejects an object payload declaration that exceeds the total envelope quota before consuming object bytes, and rejects trailing data or a truncated payload. The byte budget includes the 12-byte header, manifest transcript and all declared objects. Invalid decimal limits and unexpected extra arguments exit with usage status 2; a verification failure exits with status 1.

## Regression and CI

A new black-box integration test, `tools/wiki-pack/tests/verify_budget_cli.rs`, constructs a valid custom envelope and invokes the real CLI for both commands. It tests legacy and exact-byte success, one-byte-short rejection, invalid decimal limits, excess arguments and an appended byte. Successful output retains the explicit **publisher signature: NOT VERIFIED** warning.

- Initial implementation `0046dbb5afa3374ba6561b9b128d0403db3363ca` failed rustfmt ([Rust 41074921406](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41074921406)).
- Formatting correction `8e7ed5fe47cadedc438fc311d2d59a203c60b7b9` failed strict Clippy for a redundant test import ([Rust 41075054741](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41075054741)).
- Import correction `9cd484aaaa31a73e7d4993b0be345618c915a175` failed only rustfmt for a blank line ([Rust 41075224100](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41075224100)).
- Final corrected source `febc48edbdefba81d22638afaab03f7a4d4b943a` passed [Rust workspace 41075315355](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41075315355), including format, strict Clippy and workspace tests. [Platform shells 41075315248](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41075315248) was in progress when this report was prepared.

## Remaining acceptance

The optional ceiling is a processing budget, not a disk-space reservation, publisher-authentication check or offline-media completeness audit. The no-budget CLI forms intentionally retain existing behavior. Production pack build/install, official catalog signatures, revision-matched media, offline search/rendering and physical-device qualification remain outstanding. **PACK-004 and TRUST-003 remain unchecked.**
