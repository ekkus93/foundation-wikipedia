# RAG-003 — Deterministic preregistered article sampling

**Date:** 2026-11-03  
**Status:** Implemented local sampling helper; RAG-003 remains unchecked.

## Purpose and contract

`scripts/sample_rag003_articles.py` is a reproducible *pre-review* source-revision
selection tool for the independent RAG-003 qualification protocol. It does not
download, authenticate, or synthesize Wikimedia articles. The operator must
first provide a real candidate inventory and publish the sampling plan,
including chosen projects, required strata, random seed, exclusions and
minimum distinct articles per project, before examining model scores.

The input plan is a JSON object with exactly:
- `schema_version: 1`, `sampling_seed` (unsigned 64-bit integer);
- `required_projects` (distinct Wikimedia project IDs);
- `strata` (distinct nonempty labels);
- `minimum_articles_per_project` (positive integer);
- optional `excluded_page_keys`, each `{"project":"enwiki","page_id":42}`.

The candidate inventory is a JSON array with distinct project/page identities;
each entry has `project`, positive integer `page_id` and `revision_id`, plus
a nonempty `strata` array drawn from the plan. Competing revisions of a page
are rejected, not silently chosen.

Run:

```sh
python3 scripts/sample_rag003_articles.py \
  --plan preregistered_plan.json \
  --inventory pinned_candidate_inventory.json \
  --output selected_roster.json
```

The sampler selects each project's strata coverage greedily (preferring
candidates covering more as-yet-unrepresented labels), then fills the
preregistered distinct-article floor. SHA-256 of the seed and canonical
project/page/revision identity determines tie order, independent of input
order or Python random-library implementation. It fails closed for missing
strata, too few candidates, unplanned projects, duplicate identities, malformed
inputs and unsafe exclusions. Existing output files are never overwritten.

The compact JSON roster lists exact selected project/page/revision IDs, labels,
counts, SHA-256 of the **exact input plan and inventory bytes** and
`selection_policy: greedy-strata-sha256-v1`. Use the plan receipt in the
review-manifest preregistration binding and archive the roster before model
score inspection. The output does not prove plan publication time.

## Tests and limits

`python3 -m unittest discover -s scripts -p 'test_sample_rag003_articles.py'`
exercises deterministic order, required project/stratum coverage, exclusion,
duplicate or competing revisions, malformed inputs, byte budgets, receipts and
no-clobber output. Rust workspace CI also discovers the new Python test.

The strata are caller-supplied labels; there is no independent publisher,
label or sampling-frame authentication here. This is not a representative
Wikimedia acquisition, an independently human-reviewed corpus, factual
entailment evidence, or desktop/Android device acceptance. **RAG-003 remains
unchecked** pending genuine independently verified inputs and the qualification
protocol in `docs/RAG_003_INDEPENDENT_QUALIFICATION_PROTOCOL_2026-11-03.md`.
