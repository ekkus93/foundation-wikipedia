# SRC-003 — Official pinned member URL binding (partial qualification)

**Date:** 2026-10-25  
**Source code SHA:** `177785696e69daba657aed989fb6d52fc8fe6ddc`  
**Scope:** `crates/wiki-source/src/public_dump_pinned.rs`  
**Status:** Partial. SRC-003 remains unchecked.

## Implemented

`PinnedInventory::official_inventory_url()` derives the canonical HTTPS SHA256SUMS location from the parsed, validated Wikimedia project and calendar-day release. `PinnedInventory::official_member_url(index)` derives the corresponding official bzip2 shard location from the exact inventory member at that index, returning `None` for an undeclared index. It does not accept caller-controlled filenames or digest overrides; the member's expected SHA-256 remains accessible through `members()[index]`.

The generated URLs preserve safe, already-validated nested relative member paths and are deterministic across inventory ordering. This reduces the risk of accidentally fetching a shard unrelated to the verified checksum inventory while connecting the public-dump fallback to an explicit official source location.

## Evidence

- **Rust workspace:** [40177686395](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40177686395), success at exact source SHA, including format, lint, unit tests, synthetic retrieval, codec and staging-verifier gates.
- **Platform shells:** [40177686428](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40177686428), success at the same exact SHA; desktop frontend/Tauri and Android shell jobs passed.
- **Regression:** `official_member_urls_are_bound_to_digest_pinned_inventory_members` checks canonical checksum URL, both selected member URLs including a nested relative path, invalid index rejection, and the associated pinned SHA-256 values.
- Earlier source commits `f39ced722bff1889df43cedef91f4c24a5503b01` and `2e8220cfce79c59e36d3e7b094c63eabfd3d3334` were corrected before qualification; the latter failed the Rust formatting gate. Do not cite either as passing evidence.

## Non-claims and next dependencies

A syntactically official URL does not establish certificate-verified HTTPS retrieval, independently authenticated publisher origin, source-generation completeness, expected census independence/cutoff, or verification of the downloaded bytes. The caller must still obtain and authenticate publication metadata independently, fetch via a TLS-verified transport, hash every complete compressed member against the pinned inventory, validate page/revision semantics, and compare against an independently sourced expected census. Revision-matched rendered HTML, required visual media, installable packs, and real desktop/Android offline acceptance are still outstanding. No official-source or completed-pack claim is made here.
