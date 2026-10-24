# RAG-001 — Unicode CJK lexical retrieval and evidence budget

**Date:** 2026-10-24  
**Scope:** Partial RAG-001 / STORE-004 qualification; not an offline pack release.

## Implementation

The shared `wiki-search` lexical prototype now generates adjacent character
bigrams for contiguous Han, Hiragana/Katakana, and Hangul text. It also
retains individual CJK characters in indexed passages, so one-character
queries continue to work. Multi-character CJK query terms use bigrams
rather than unrelated single-character hits; e.g. the query `引力` does
not match the passage `引 和 力`.

This logic applies to both revision-scoped article/corpus evidence search
and navigation-only title/alias search. Latin word tokenization and title
ranking remain supported.

Previously, the word-budgeted evidence selector counted a long unspaced
CJK passage as a single word. The new budget cost counts each CJK Unicode
scalar separately, while preserving ordinary space-delimited word costs.
This is a conservative word-cost heuristic, **not** an LLM tokenizer or
a guaranteed exact model-context token count.

## Regressions and exact-head evidence

- Code commit: `cbf69af100fce16d0ae62efa6f65d0b912b079cb`.
- Chinese `相对论`, Japanese `量子力学`, Korean `역학` lexical
  evidence retrieval; Chinese/Japanese/Korean title and alias navigation.
- A separated-character negative fixture prevents an accidental unigram-only
  match for the two-character `引力` query.
- Both single-article and corpus-wide evidence selection reject an
  unspaced six-character passage under a five-word budget and admit it
  under a six-word budget.
- [Rust workspace 40079126624](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40079126624): passed.
- [Platform shells 40079126682](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40079126682): passed.
- [Android Rust ABI 40079126684](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40079126684): passed.

## Remaining acceptance

The index is still a small in-memory lexical prototype, not a persistent
SQLite/Tantivy pack index, language-specific word segmenter, or proven
multilingual relevance benchmark. Its approximate word cost is not a
guaranteed model-token bound. Source authenticity, installed pack media,
desktop/Android UI integration, and physical offline tests remain open.

**RAG-001 and STORE-004 remain unchecked.**
