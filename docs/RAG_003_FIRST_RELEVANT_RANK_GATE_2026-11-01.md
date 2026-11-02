# RAG-003 — First relevant evidence rank regression gate

**Date:** 2026-11-01 (America/Los_Angeles)  
**Repository:** `ekkus93/foundation-wikipedia`  
**Verified source SHA:** `ec4bbecf54f93f9713c877181260229ddffcfe5b`  
**Status:** Partial; **RAG-003 remains unchecked**.

## Motivation

Aggregate recall and per-case recall/precision can pass when the only labeled relevant evidence appears late in the retrieval ranking. For an article-grounded assistant using a bounded context budget, early relevant evidence matters. This gate evaluates ranking order without pretending to measure factual entailment.

## Implementation

The `wiki-search --example retrieval_suite_runner` accepts `--max-first-relevant-rank N`, with `N=0` disabling the gate for backward compatibility and `N=1..100` requiring every answerable case to retrieve a gold-labeled block within the first N hits. The gate is evaluated over the shared Rust retrieval evaluator's exact revision-scoped, ranked evidence receipts, not by comparing untrusted free-text snippets.

When enabled, the runner rejects missing ranked evidence, noncontiguous rank ordinals, disagreement between ranked-hit count and retrieved count, or disagreement between gold-labeled hits and the evaluator's found count. A case with no relevant hit fails; a suite with no answerable cases cannot pass vacuously. No-answer false positives remain governed by the separate existing no-answer gate. The report retains schema v3 and adds `first_relevant_rank_gate`, including the configured rank, answerable-case count, each observed first relevant rank (or null), failed case IDs, and pass/fail status. A failed quality gate still publishes the full report atomically without overwriting existing evidence.

Example invocation (with caller-supplied, unauthenticated data):

```sh
cargo run -p wiki-search --example retrieval_suite_runner --locked -- \
  --corpus articles.json --suite labels.json \
  --sha <exact-40-hex-commit> --output retrieval-report.json \
  --top-k 5 --max-first-relevant-rank 2
```

The `--sha` value remains a caller assertion; the runner does not authenticate repository identity, publisher provenance or gold labels.

## Automated evidence

The new unit regression demonstrates a case where the first relevant block is ranked third, another is ranked first, and a third answerable case retrieves no gold. It checks failure IDs and observed ranks, boundary 0/101 handling, missing/duplicated rank receipts, and no-answer-only suites. The existing disk-backed SQLite snapshot test now exercises a successful first-rank gate and a failed no-gold case with a preserved diagnostic report. CLI tests cover invalid and duplicate options.

- Initial source commit `7ca923edc6334f123ff505f71a444bc90b0d2b80`: Rust workspace format check failed; the implementation was corrected.
- Corrected source commit `ec4bbecf54f93f9713c877181260229ddffcfe5b`: [Rust workspace 41396589986](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41396589986), [Platform shells 41396589961](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41396589961), and [Android Rust ABI 41396589963](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41396589963) all passed.

## Limitations and next steps

This is an **offline, caller-labeled retrieval** ranking gate. It does not prove a passage supports an AI-generated claim, measure generated citation precision, authenticate the Wikimedia source or label authors, establish representative real-article question quality, or benchmark a physical Android device. Those RAG-003 acceptance criteria remain open. AI-005 desktop article-grounded inference and verified citation UI remain a separate higher-level integration milestone.
