# PACK-004 / TRUST-003 — Seekable staging transcript stability (partial)

**Date:** 2026-10-30 (America/Los_Angeles)  
**Implementation:** `48976e9a4d63338a88f27019b10409ea4ad27342`  
**Scope:** Provisional unsigned `.wpack` two-pass staging, not an installer or publisher attestation.

## Problem

The seekable staging path verified the entire input envelope and then sought directly to a manifest length read from the same source a second time. A mutable source could replace the manifest between passes while keeping object payloads identical. The second pass rehashed staged objects, but did not compare the manifest identity against the initially verified manifest, so the staged tree could be attributed to the wrong pack identity. A changed second-pass length could also move the payload offset.

## Implementation

- Rewind after the initial full verification and re-read the fixed framing header.
- Require the same magic, exact canonical manifest transcript length, and byte-for-byte canonical transcript. Compare in bounded 64 KiB chunks before creating the staging destination.
- Continue to verify all staged object hashes, lengths, complete object inventory and absence of trailing bytes. Existing failure cleanup remains in effect.
- Add a regression using a seekable source that swaps between two individually valid, equal-length pack envelopes with different pack IDs but identical payloads; the staging directory must not be created.
- Update the existing appended-tail mutation regression to inject on the second seek rather than assuming a particular seek offset.

## Exact-head qualification

- [Rust workspace — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41050826803) (format, strict Clippy and tests).
- [Platform shells — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41050826844).
- [Android Rust ABI qualification — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41050826850).

Two intermediate commits exposed and corrected a formatting issue and a test fixture coupled to the previous seek offset. The corrected implementation and regression test are present together in the cited passing source SHA.

## Qualification boundary

This protects against changes in the manifest and framing between passes on a seekable source. It does not prevent a hostile concurrent writer from racing the staging filesystem, prove upstream source authenticity, verify publisher signatures, establish complete offline media, or demonstrate a real Android document-provider install. **PACK-004 and TRUST-003 remain unchecked.**
