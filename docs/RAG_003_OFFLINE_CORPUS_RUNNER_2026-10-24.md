# RAG-003 — Offline Canonical Corpus Benchmark Runner (2026-10-24)

## Implementation

Added `crates/wiki-search/examples/retrieval_suite_runner.rs`, a command-line executable for evaluating a versioned retrieval suite against a **caller-supplied canonical article JSON array**. It reuses `CorpusLexicalIndex::build` and the strict `run_retrieval_suite_json` library gate, so gold references must exist in the exact indexed article revision. It writes the deterministic v2 report before returning a nonzero exit status on a regression.

Example:

```sh
cargo run -p wiki-search --example retrieval_suite_runner --locked -- \
  --corpus articles.json --suite labels.json \
  --sha <exact-40-character-commit> --output report.json \
  --top-k 5 --min-recall-per-mille 800 --max-no-answer-fp 0
```

Input is bounded to 64 MiB/20,000 canonical articles and 2 MiB for the suite, with explicit top-k and threshold validation. The `--sha` is a caller assertion; CI must separately confirm that the checked-out code corresponds to it. This runner makes it possible to evaluate larger exported corpora without recompiling a hard-coded synthetic fixture.

## Remaining

The runner **does not** read `.wpack` files directly, establish official Wikimedia provenance, prove source completeness or validate human relevance labels. Its corpus is caller supplied and the existing synthetic CI gate remains synthetic. A real-pack exporter, authenticated Wikimedia corpus and labels, factual answer-support grading, device performance and real-pack CI baselines are still outstanding. **RAG-003 remains unchecked.**
