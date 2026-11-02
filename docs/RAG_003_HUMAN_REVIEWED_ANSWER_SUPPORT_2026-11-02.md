# RAG-003 — Human-reviewed answer support and citation precision

**Date:** 2026-11-02 (America/Los_Angeles)  
**Implementation SHA:** `28fb35698bbf6ca82fd5f9a78ad4355c293b9b22`  
**Status:** Partial implementation; RAG-003 remains unchecked.

## Scope and motivation

Retrieval relevance is not factual entailment: an article passage can match a question without supporting a particular generated assertion. `wiki_search::answer_support::evaluate_answer_support_json` adds a separate, deterministic measurement primitive for **explicitly human-reviewed** generated answers. It deliberately does not reuse lexical retrieval relevance as a proxy for support.

This is a library API (not yet a dedicated CLI). The caller supplies a bounded JSON schema-v1 suite with a stable suite ID and at least one case. Each case contains exact Wikimedia project/page/revision identity, question, literal generated answer and nonempty claim annotations. Each claim includes literal text appearing within the answer, its reviewer verdict, and per-citation reviewer verdicts bound to `[[cite:BLOCK_ID]]` markers in that answer.

Example input (illustrative reviewer annotations, **not** an authenticated Wikimedia evaluation):

```json
{
  "schema_version": 1,
  "suite_id": "review_round_1",
  "cases": [{
    "id": "example_1",
    "project": "enwiki",
    "page_id": 42,
    "revision_id": 100,
    "question": "How many moons?",
    "answer": "Two moons [[cite:lead_0]].",
    "claims": [{
      "id": "claim_1",
      "text": "Two moons",
      "verdict": "supported",
      "citations": [{"block_id": "lead_0", "verdict": "supported"}]
    }]
  }]
}
```

## Contract

- Rejects oversized suites (2 MiB), zero or excessive cases, invalid identities, overly long answers/questions, duplicate case and claim IDs, duplicate citations within a claim, malformed citation IDs, missing declared citation markers, and answer markers absent from the labels.
- Requires at least one individually supported citation for every claim labeled `supported`. A claim can be `unsupported` or `unreviewed` even if a citation exists.
- Counts reviewed/supported/unsupported/unreviewed **claims** and reviewer-judged **claim/citation pairs** separately. An unreviewed judgment is excluded from the reviewed denominator, never treated as supported. `all_claims_reviewed` and `all_citations_reviewed` indicate whether supplied annotations are complete, without claiming the annotator discovered every possible assertion in an answer.
- Uses integer numerators/denominators rather than rounded percentages. Claim support = `supported_claims / reviewed_claims`; citation precision = `supported_citations / reviewed_citations`. A zero denominator is **undefined**, not 100%.
- Includes per-case SHA-256 of the literal answer text and overall SHA-256 of the exact raw input bytes to help reproduce reviews. Cases are sorted by ID.

## Automated evidence

Rust tests cover positive support and deliberately unsupported citations, unreviewed judgments, malformed/missing/unlabeled markers, invalid article identities and schema, incongruent support verdicts, and suite-size limits.

- [Rust workspace — run 41413319038](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41413319038): passed, including formatting, Clippy, workspace tests and retrieval regression.
- [Android Rust ABI — run 41413318989](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41413318989): passed.
- [Platform shells — run 41413318959](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41413318959): in progress at documentation.

Initial code commit `f4dc5e5883c11af6edeff305f3cd63e1bad5afe9` exposed an invalid struct literal; subsequent corrections repaired that and test assertions. The source commit above passed the Rust qualification.

## Limitations and next steps

This scorer cannot authenticate Wikimedia source bytes, verify that a cited block belongs to the current article, prove that a human's support judgment is correct, infer entailment automatically, or guarantee that every factual claim in the answer was annotated. Cite the existing revision-safe core validation separately; do not promote historical chat citations on the basis of this scorer.

Next: expose safe CLI/report generation for reviewed answer suites, assemble independently reviewed Wikimedia-revision fixtures with traceable evidence and reviewer identity, establish score thresholds and baseline reports at exact commits, then measure realistic device memory/latency. Desktop AI-005 chat generation with canonical evidence and explicit consent remains a separate prerequisite for end-to-end qualification.
