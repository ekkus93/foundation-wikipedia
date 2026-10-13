# SRC-003 public-dump staging sidecar parity — 2026-10-13

Status: **qualified partial implementation, not full SRC-003 acceptance**.

The digest-pinned public-dump exact-tree verifier previously rejected the paired `SHA256SUMS` and `PUBLICATION.json` sidecars permitted by its underlying staged-export verifier. The two verifiers disagreed about a valid staging tree.

The fix in `crates/wiki-source/src/public_dump_strict.rs` preserves strict member digest and exact-tree verification while accepting only the paired publication sidecars. The regression test covers a verified member without sidecars, a lone sidecar (rejected), the pair (accepted), and an unexpected file (rejected).

Qualification: initial implementation commit `34490fb1b89216a919fa5093e7cedc80fcb44ec3` failed Rust formatting. Corrected implementation `891c2a10c505fd86d6253bf05c0d22ee05704bb6` passed exact-head Rust workspace CI run 38378622349 and Platform shells CI run 38378621489.

This does **not** authenticate a Wikimedia publication or complete XML import, renderer fidelity, offline pack generation, or desktop/Android acceptance. **SRC-003 remains unchecked** in the canonical `docs/WIKIPEDIA_AI_READER_TODO.md`. This standalone evidence is provided until the canonical checklist can be reconciled without transmitting its full 250+ KiB body through the current write interface.
