# RAG-003 — Whitespace-normalized reviewed-case replay guard (2026-11-02)

## Scope

Exact duplicate case detection previously compared question and answer strings byte-for-byte. The same reviewed answer could be submitted under a second case ID with harmless changes in spaces, tabs or newlines, thereby counting the same example twice in aggregate benchmark metrics.

## Implementation

`crates/wiki-search/src/answer_support.rs` now keys the duplicate-case check by project, page ID, revision ID, and Unicode-whitespace-normalized question and answer text. Normalization collapses runs of Unicode whitespace to one ASCII space and strips leading/trailing whitespace. It is used **only for duplicate detection**: the original answer, exact UTF-8 claim spans, citation positions and answer SHA-256 remain untouched.

Regression tests exercise annotation schemas v1, v2 and v3, including whitespace-only replay under another case ID and a legitimately different question. This does not detect semantic paraphrases or certify reviewer independence.

## Qualification

Corrected source SHA: `3e732ccb0733cab6d8ce09ded1abe11aec7ddd4f`. Exact-head CI passed:
- Rust workspace (format, Clippy, tests including executable benchmark tests): https://github.com/ekkus93/foundation-wikipedia/actions/runs/41532284643
- Platform shells: https://github.com/ekkus93/foundation-wikipedia/actions/runs/41532284663
- Android Rust ABI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/41532284560

Initial source SHA `a1ea7265a96efcf5dd9151764dd57fe15b5d2ba9` failed rustfmt (Rust workspace run 41532044361) and was superseded.

**RAG-003 remains unchecked:** representative independently reviewed Wikimedia datasets, authenticated provenance, factual-entailment qualification and real-device acceptance are still outstanding.
