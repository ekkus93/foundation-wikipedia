# RAG-003 — Whitespace-normalized reviewed-case replay guard (2026-11-02)

## Scope

Exact duplicate case detection previously compared question and answer strings byte-for-byte. The same reviewed answer could be submitted under a second case ID with harmless changes in spaces, tabs or newlines, thereby counting the same example twice in aggregate benchmark metrics.

## Implementation

`crates/wiki-search/src/answer_support.rs` now keys the duplicate-case check by project, page ID, revision ID, and Unicode-whitespace-normalized question and answer text. Normalization collapses runs of Unicode whitespace to one ASCII space and strips leading/trailing whitespace. It is used **only for duplicate detection**: the original answer, exact UTF-8 claim spans, citation positions and answer SHA-256 remain untouched.

Regression tests exercise annotation schemas v1, v2 and v3, including whitespace-only replay under another case ID and a legitimately different question. This does not detect semantic paraphrases or certify reviewer independence.

## Qualification

Source changes must pass the exact-commit Rust workspace (including `cargo test -p wiki-search --example answer_support_runner --locked`), Platform shells and Android ABI workflows. Record passing run URLs before claiming CI qualification.

**RAG-003 remains unchecked:** representative independently reviewed Wikimedia datasets, authenticated provenance, factual-entailment qualification and real-device acceptance are still outstanding.
