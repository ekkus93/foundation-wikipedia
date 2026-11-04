# RAG-003 sampling receipt replay verification (2026-11-03)

**Status:** Local deterministic replay verifier implemented; RAG-003 remains unchecked.

After running `scripts/sample_rag003_articles.py` with a preregistered
plan and pinned inventory, a separate operator can replay and check the
complete selected-roster receipt:

```sh
python3 scripts/verify_rag003_sample_roster.py \
  --plan preregistered_plan.json \
  --inventory pinned_candidate_inventory.json \
  --receipt selected_roster.json
```

The verifier recomputes the selection from the supplied original bytes,
compares the exact plan/inventory SHA-256 receipts, every selected
project/page/revision identity and label, ordering, coverage counts,
policy version and all remaining fields. It fails closed on missing or
altered selections, mutated input bytes, duplicate JSON keys, extra
claims, boolean/int substitution, invalid inputs and oversized files.
The replay is read-only and does not replace the original roster.

Run `python3 -m unittest discover -s scripts -p
'test_verify_rag003_sample_roster.py'` for synthetic positive and negative
regressions; the Rust workspace workflow runs Python test discovery.

**Boundary:** Matching a receipt to two local files does not prove that
those files existed at the claimed preregistration time, that Wikimedia
published them, that their strata labels are correct, or that human
reviewers are independent. The real evaluation, independent source
authentication and device acceptance remain outstanding.
