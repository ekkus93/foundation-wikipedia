# RAG-003 — Review manifest to preregistered sample binding

**Date:** 2026-11-04  
**Status:** Partial; RAG-003 remains unchecked.

The deterministic sampling roster, its replay verifier and the semantic
independent-review manifest previously operated independently. A review
manifest might therefore include revisions outside the selection, omit
sampled revisions or substitute another revision without failing its
own semantic checks.

`scripts/verify_rag003_review_selection.py` closes this cross-artifact gap.

```sh
python3 scripts/verify_rag003_review_selection.py \
  --manifest independent_review_manifest.json \
  --plan pinned_sampling_plan.json \
  --inventory pinned_candidate_inventory.json \
  --receipt selected_roster.json \
  --evidence-root evidence
```

The gate validates the full existing review-manifest JSON schema and semantic
rules, optionally verifies every required source and evaluation file's actual
SHA-256 bytes, replays selection using the exact pinned plan/inventory bytes,
compares the complete deterministic receipt, and enforces identical
`plan_sha256`, sampling seed, projects, strata and minimum articles per
project across plan and review manifest. Each source record must match
exactly one selected project/page/revision, without additions or omissions.

Synthetic regression covers a valid bundle, unselected and substituted
revisions, omitted sampled pages, plan metadata drift, receipt/inventory
tampering, duplicate JSON keys, and CLI success/failure behavior.

This is a *local cross-artifact integrity check*, not proof of an independently
published preregistration timestamp, a representative or authenticated
Wikimedia sample, genuine independent human review, factual entailment,
or physical-device acceptance. Those qualifications remain required by
`RAG_003_INDEPENDENT_QUALIFICATION_PROTOCOL_2026-11-03.md`.
**RAG-003 remains unchecked.**
