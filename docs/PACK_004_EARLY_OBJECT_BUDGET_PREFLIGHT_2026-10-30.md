# PACK-004 / TRUST-003 — Early bounded envelope payload preflight (partial)

**Date:** 2026-10-30 (America/Los_Angeles)  
**Scope:** Provisional unsigned `.wpack` stream verifier; not an installer or publisher signature verifier.

## Problem

The bounded nonseekable verifier already rejected an oversized manifest transcript after the fixed 12-byte header. However, a canonical manifest declaring more object payload bytes than the caller's quota could still trigger reads until that quota was exhausted, even though the envelope could never fit.

## Implementation

- Refactor the shared verifier into a private implementation with an optional full-input quota; the public unbounded verifier retains its existing API.
- After canonical manifest validation, sum the header, transcript and all declared object sizes with checked arithmetic. Return `ResourceBudget` before consuming any object payload when the declared total exceeds the limit.
- Retain streamed SHA-256 object verification, truncation detection, the actual byte-budget reader and trailing-data rejection; declared lengths do not replace byte-level verification.
- Add synthetic regressions for an oversized declared object without payload consumption, exact-budget success, and one-byte-short rejection.

## Qualification boundary

This is an input-budget resource guard for a future Android SAF integration, not a real document-provider or physical-device test. It does not establish publisher signatures, offline media completeness, atomic installation or power-loss recovery. **PACK-004 and TRUST-003 remain unchecked.**

Exact-head Rust and platform CI are required; the commit itself is not proof of passing tests.
