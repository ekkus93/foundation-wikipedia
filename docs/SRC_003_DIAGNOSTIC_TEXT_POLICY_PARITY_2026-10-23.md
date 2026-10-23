# SRC-003 — Diagnostic XML/JSONL label-policy parity (partial)

Date: 2026-10-23  
Status: Qualified partial implementation in Rust workspace CI; **SRC-003 remains unchecked**.

The Python public-dump bzip2/XML diagnostic decoder and the independent
diagnostic JSONL auditor now share a bounded label validator matching the
Rust `wiki-source::public_dump_page_meta::valid_text` contract. Titles and
redirect targets reject Unicode C0/C1 controls, invisible zero-width space,
directional-format controls, BOM/word joiner, and Unicode noncharacters in
the BMP and supplementary planes. Persian ZWNJ and Indic ZWJ are preserved.
The JSONL auditor also rejects XML-forbidden wikitext even if the record's
self-reported SHA-256 matches, and requires the Rust-compatible Wikimedia
project identifier shape.

Cross-adapter synthetic tests cover both title and redirect fields, real
multilingual labels, XML text controls, and project identity boundaries.
Validation command: `python3 -m unittest discover -s scripts` (also run by
the Rust workspace CI workflow).

This closes a disagreement between the Python diagnostic entry points and
the Rust semantic metadata/JSONL readers; it is not a portable Rust bzip2/XML
importer, authenticated upstream dump, independent expected census, rendered
Parsoid fidelity, mandatory offline media, or desktop/Android acceptance.
Do not mark SRC-003 complete on this basis.

## Exact-head qualification

Source SHA `97fbe40123f509ec575c1fa43bd6f2caf7f1b436`:

- Rust workspace CI [39933200057](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39933200057) — passed formatting, strict lint, tests and source verification.
- Platform shells CI [39933199964](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39933199964) — running at time of this documentation update; check its final result before claiming full cross-platform qualification.
