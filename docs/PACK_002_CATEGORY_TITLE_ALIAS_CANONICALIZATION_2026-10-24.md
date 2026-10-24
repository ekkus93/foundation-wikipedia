# PACK-002 — Category underscore/space identity in local topic previews

**Date:** 2026-10-24  
**Scope:** Partial category graph identity correctness. PACK-002 remains unchecked.

## Problem

MediaWiki title keys treat underscores and spaces as equivalent. The
revision-joined Enterprise category graph previously keyed category pages and
memberships by their raw spellings, and the standalone `wiki-pack preview-topic`
JSON CLI did the same. A source category `Category:Quantum_mechanics` and a
membership `Quantum mechanics` could be treated as different graph nodes,
causing a missing-category error or an incorrect topic selection. Conversely,
two aliases could be accepted as distinct category pages.

## Changes

- `crates/wiki-pack-builder/src/enterprise_graph.rs` canonicalizes underscores
  to spaces before indexing category page names and references. Alias collisions
  are rejected as duplicate categories or memberships. Administrative policy
  must name the canonical category so an intended skip cannot silently fail.
- `tools/wiki-pack/src/preview_topic.rs` applies the same canonical spelling
  to raw JSON roots, category nodes and child links before traversal, hashing
  and duplicate checks. Noncanonical leading/trailing underscore aliases fail
  validation. The local input remains explicitly untrusted.
- Typed graph tests cover cross-spelling membership, canonical administrative
  exclusions, duplicate page/alias identities and malformed underscore labels.
- CLI tests cover cross-spelling root and child edges, duplicate canonical
  category nodes and invalid normalized names.

## Exact-head evidence

- Typed-graph implementation: `0d8bdb6ed062a3a650b82aec9b3aa6f74e03d874`.
  [Rust workspace #40074639870](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40074639870)
  passed on attempt 2. Attempt 1 failed an unrelated intermittent
  `wiki-store/tests/activation.rs` pointer-preservation assertion; no change
  to activation code was made.
  [Platform shells #40074639865](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40074639865)
  passed.
- CLI implementation: `8a985fa255fb4498e22c79a12c35c95ff92e3485`.
  [Rust workspace #40074824030](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40074824030)
  failed only rustfmt on an extra blank line.
- Corrected CLI: `fced64a8de207f1fc849d5695573db22d8b1d5e0`.
  [Rust workspace #40074875094](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40074875094)
  passed formatting, Clippy and workspace tests.
  [Platform shells #40074875117](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40074875117)
  was in progress at documentation time.

## Remaining acceptance

The canonicalization covers underscore/space spelling only, not all
language-specific MediaWiki title rules. Inputs remain caller-supplied and
untrusted; category source completeness, official publication authenticity,
real source category fan-out, a media-complete pack build, installed-pack
navigation and desktop/Android offline acceptance remain unverified.
