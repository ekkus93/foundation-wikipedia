# PACK-002: Typed Enterprise graph boundary validation

**Date:** 2026-10-23  
**Status:** Partial qualification; PACK-002 remains unchecked.

## Scope and behavior

`wiki-pack-builder::enterprise_graph::graph_from_joined` accepts a public
`JoinedEnterpriseArticle` DTO. A caller can construct that DTO without using
the strict `wiki-source` Enterprise JSON parser. The graph builder therefore
rechecks the canonical project identifier and conservative generation ID,
and applies the source parser's bounded, anti-spoof membership-label policy
to category references, category page titles and redirect title aliases.
This includes non-mainspace records that do not contribute graph edges.
Malformed category fragments are rejected. Legitimate multilingual labels
and stable joined category membership remain supported.

Adversarial tests live in
`crates/wiki-pack-builder/src/enterprise_graph_hardening_tests.rs` so the
existing graph module retains its original test coverage and stays small.

## Evidence

- Initial code SHA `c5e843b3cf618bb160845f90dd1e9b2e483c03f1` and syntax
  follow-up `4296e3b70227adae256423fe939a391927f2df11` failed Rust format:
  a large inline test rewrite was truncated before the end of the module.
- Restored complete source and split regressions: exact SHA
  **`5e36bda80c589e06040652ab753080595909a492`**.
- [Rust workspace 40040686827](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40040686827)
  **passed** formatting, strict Clippy and workspace tests.
- [Platform shells 40040686816](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40040686816)
  **passed** desktop frontend, Android shell and desktop Tauri checks.
- Negative cases: malformed project/generation; category whitespace,
  directional-format spoofing, excessive bytes and fragment; redirect
  whitespace, directional-format spoofing and excessive bytes.
- Positive case: a legitimate Unicode redirect alias and category membership
  from a synthetic revision-joined batch.

## Remaining

This only checks local typed-input integrity. Source generation authority,
complete category/redirect coverage, deterministic end-to-end source selection,
all required media, pack creation/install and physical offline qualification
remain open. Do not mark PACK-002 or SRC-002 complete on this evidence alone.
