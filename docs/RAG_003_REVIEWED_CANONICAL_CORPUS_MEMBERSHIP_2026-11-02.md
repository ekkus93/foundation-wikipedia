# RAG-003 — Strict canonical article membership for reviewed answer citations

**Date:** 2026-11-02 (America/Los_Angeles)  
**Exact passing source SHA:** `5db40aff5419ab970709b89698d86762779da364`  
**Status:** Partial; RAG-003 remains unchecked.

## Problem and scope

The prior reviewed-answer evaluator checked the syntax and project/page/revision embedded in `wkb:` IDs, but a fabricated block ordinal for that same article/revision could still be positively annotated. A structurally valid ID is not proof of a block in the canonical article.

## Implementation

- `crates/wiki-search/src/answer_support.rs` adds `evaluate_answer_support_with_corpus_json(input, articles)`. It runs existing label/marker validation, builds validated `ArticleLexicalIndex` instances for each supplied article, rejects duplicate article keys, requires exact revision identity, and verifies every cited ID against an actual retrievable block. Synthetic single-token IDs cannot pass strict mode. HTML-only fallbacks are not silently treated as lexical evidence.
- `crates/wiki-search/examples/answer_support_runner.rs` accepts optional `--corpus canonical_articles.json`. Strict mode reads canonical `Vec<Article>` JSON input once with a 64 MiB read cap and a 1,000-article limit, records SHA-256 of the raw corpus bytes and `evidence_membership_checked: true` in the report, and refuses to publish a report if a cited block is missing or stale. The legacy no-corpus mode remains for exploratory tests and explicitly reports that membership was not checked.
- Library regressions cover valid lead/section IDs, forged but structurally valid IDs, stale revisions, duplicate/malformed articles, fixture aliases, missing corpus and HTML-only evidence. A CLI end-to-end regression verifies a successful strict report and refusal to publish when the corpus revision changes.

Example (corpus must be an array of canonical `wiki_model::Article` JSON objects):

```bash
cargo run -p wiki-search --example answer_support_runner --locked -- \
  --suite reviewed_answers.json --corpus canonical_articles.json \
  --output reviewed_report.json \
  --min-claim-support-per-mille 800 \
  --min-citation-precision-per-mille 800
```

## Exact-head verification

- [Rust workspace 41425459296](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41425459296): **passed**, including format, strict Clippy and tests.
- [Platform shells 41425459329](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41425459329): **passed**.
- [Android Rust ABI 41425459326](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41425459326): **passed**.

Earlier intermediate commits failed formatting or lint and were corrected before this exact passing source SHA.

## Remaining

The corpus and reviewer labels are caller-supplied; the hash is a replay receipt, not source authentication. The strict mode does not verify that a cited passage was actually supplied to the generating model or that it entails the claim. Independently reviewed Wikimedia cases, publisher-verified article provenance, representative scoring baselines, chat integration and real-device acceptance remain outstanding. Keep RAG-003 unchecked.
