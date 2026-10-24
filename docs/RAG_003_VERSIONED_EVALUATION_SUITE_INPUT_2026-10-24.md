# RAG-003 — Versioned Evaluation Suite Input (2026-10-24)

## Scope and API

Added `wiki_search::evaluation_suite::parse_retrieval_suite_json`, an offline-only parser for version-1 labeled lexical-retrieval benchmark suites. The public `RetrievalSuite` holds a stable suite ID and sorted `RetrievalCase` values for the existing `evaluate_retrieval` and `evaluate_retrieval_report_json` APIs.

A suite is JSON with `schema_version: 1`, a conservative ASCII `suite_id`, and a nonempty `cases` array. Each case supplies `id`, `query`, optional `project`, and `relevant` (an explicit array of gold evidence). Each gold item supplies `article: {project, page_id}`, `revision_id`, and `block_id`. An empty `relevant` array explicitly labels a no-answer retrieval case.

```json
{
  "schema_version": 1,
  "suite_id": "synthetic-physics-v1",
  "cases": [
    {
      "id": "physics",
      "query": "gravity",
      "project": "enwiki",
      "relevant": [
        {
          "article": {"project": "enwiki", "page_id": 9},
          "revision_id": 12,
          "block_id": "wkb:enwiki:9:12:b0"
        }
      ]
    }
  ]
}
```

## Fail-closed constraints

- The JSON input is capped at 2 MiB, with at most 1,000 cases and 128 gold items per case. Query bytes follow the shared lexical query cap.
- Unknown fields and duplicate JSON object keys (including nested page identity keys) are rejected, as are unsupported schema versions and malformed suite IDs.
- Duplicate case IDs and duplicate revision-scoped gold identities are errors, not silently deduplicated. Canonical sorting of cases is independent of input order.
- The parser checks canonical project/page identity, nonzero revision IDs, nonempty bounded block IDs, edition scope, and control characters in case IDs, queries and block IDs.
- **Parsing does not establish that a gold block exists**. The caller must evaluate the suite against an exact `CorpusLexicalIndex`; the evaluator rejects fabricated or stale revision-scoped gold. Neither parser nor evaluator authenticates Wikimedia source generations or human relevance labels.

## Exact-head qualification

- Initial implementation `c39e4fc1efae580c4fb0453398abaf6463415a68` failed only Rust formatting ([Rust workspace #40094547000](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40094547000)).
- Corrected source `42030b8f575a6480955ca9710a48c7be4c2c811e` passed [Rust workspace #40094604552](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40094604552) (format, strict Clippy, tests and codec probe), [Platform shells #40094604515](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40094604515), and [Android Rust ABI #40094604534](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40094604534).
- Synthetic unit tests cover canonical case order, stale gold rejection at evaluation time, unknown/duplicate keys, duplicate cases/gold, invalid scope/page/revision/query, unsupported version and input size budget.

## Still open

This is a suite-ingestion primitive, **not a representative corpus or a CI relevance gate**. RAG-003 remains unchecked until independently sourced real Wikipedia questions/labels, answer-support and citation-precision grading, persisted real-pack regression reports, CI gating and desktop/Android performance evidence are qualified. No publisher authentication, mandatory offline media or end-to-end device acceptance is claimed.
