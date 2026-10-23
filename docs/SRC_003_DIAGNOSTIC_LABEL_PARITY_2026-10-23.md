# SRC-003 — Diagnostic JSONL title/redirect Unicode parity

**Date:** 2026-10-23  
**Status:** Qualified partial improvement; SRC-003 remains unchecked.  
**Exact code SHA:** `d1617f1ba43bc174e3dc5427827d306d75f78aed`

## Change

The bounded Rust diagnostic revision-content JSONL reader previously used a less restrictive page-title/redirect validator than the streaming XML page metadata collector. As a result, a caller-supplied JSONL record could carry invisible directional formatting or Unicode noncharacters even when its wikitext SHA-256 and scoped revision identity were otherwise consistent.

`public_dump_revision_jsonl::valid_title` now calls the shared `public_dump_page_meta::valid_text` policy and retains its XML 1.0 text check. This keeps source page identity consistent between the XML semantic event collector and the JSONL bridge without rejecting legitimate language orthography: Persian ZWNJ (U+200C), Indic ZWJ (U+200D), Arabic, Hebrew and Japanese remain accepted.

The regression `diagnostic_titles_and_redirects_match_xml_unicode_identity_policy` covers both fields. It rejects U+200B, U+202E, U+2060, U+FEFF, U+1FFFE and U+10FFFF, checks the stream stays poisoned after rejection, and accepts representative multilingual labels. Existing digest, revision and census tests remain in place.

## Exact-head qualification

- [Rust workspace CI 39926092166](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39926092166): passed, including rustfmt, strict Clippy and Rust tests at `d1617f1ba43bc174e3dc5427827d306d75f78aed`.
- [Platform shells CI 39926092201](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39926092201): passed at the same exact code SHA.
- An intermediate source SHA `8546fcd0c394448bb0b67f9058daf24afde71ec6` failed Rust formatting; it is **not** qualification evidence.

## Remaining SRC-003 work

This is a deterministic synthetic-input security/fidelity regression only. It does **not** verify publisher-authenticated Wikimedia dumps, independent expected census, portable bzip2/XML ingestion, MediaWiki/Parsoid-compatible rendering, mandatory offline visuals/attribution, or real desktop/Android acceptance. Do not mark SRC-003 completed on this evidence.
