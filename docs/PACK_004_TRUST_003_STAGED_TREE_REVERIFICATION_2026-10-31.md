# PACK-004 / TRUST-003 — Read-only staged-tree re-verification (2026-10-31)

## Scope

An unsigned archive may have been verified and staged earlier, but the object tree could later be changed before downstream processing. A separate `wiki-pack verify-staged` command now permits a fresh read-only check against a caller-supplied exact `.wpack` archive. This is an integrity safeguard, not an installer or publisher-authentication mechanism.

## Usage and contract

```sh
wiki-pack verify-staged input.wpack /private/staging/candidate
wiki-pack verify-staged --json input.wpack /private/staging/candidate 104857600
```

The command validates the complete archive and its optional byte budget, then validates each declared staged object's size and SHA-256, the exact tree inventory, and the existing symlink/special-file/hardlink safeguards. It makes no changes. Successful JSON reports `schema_version: 1`, `status: "untrusted_staged_tree_verification"`, `manifest_sha256`, `measured_payload_bytes`, `staged_tree_verified: true`, and the claimed origin. `publisher_signature_verified`, `offline_media_complete`, and `installed` remain false even for an unsigned Official-origin claim. Failures print no JSON success receipt. Malformed invocations exit 2, verification failures exit 1.

## Exact implementation and CI

- Initial `f9ff4f25ab186b6568e619dc0da07c868d381433`: [Rust 41221319862](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41221319862) failed rustfmt.
- Formatted `e5f7a37d41ebba39392fd55d85d1fca1f802ea56`: [Rust 41221427558](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41221427558) failed strict Clippy on an unused test import.
- **Corrected `24c55b8e25c6c8990039a3e15c458269c6106652`:** [Rust workspace 41221517632](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41221517632) passed format, lint and tests; [Platform shells 41221517584](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41221517584) passed.

Black-box tests in `tools/wiki-pack/tests/verify_staged_cli.rs` verify success, the canonical manifest receipt, malformed requests, exact-byte-budget rejection, changed object bytes, undeclared files, symlinked staged roots, and recovery after restoring the exact contents.

## Limitations

This check assumes an exclusively owned immutable staging tree. It does not establish atomic installation, persistent authenticated trust-root signatures, mandatory image and equation completeness, or physical Android/desktop acceptance. Retaining the exact original archive is the caller's responsibility. **PACK-004 and TRUST-003 remain unchecked.**
