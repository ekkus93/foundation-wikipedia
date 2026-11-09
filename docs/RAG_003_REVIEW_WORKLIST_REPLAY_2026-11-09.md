# RAG-003 — Independent pending-worklist replay

**Status:** Partial. This verification is not qualification of RAG-003.

After generating the pending assignment worklist, replay it from all four
pinned inputs before distributing assignments:

```sh
python3 scripts/verify_rag003_review_worklist.py \
  --plan pinned_sampling_plan.json \
  --inventory pinned_candidate_inventory.json \
  --receipt selected_roster.json \
  --reviewers reviewer_ids.json \
  --worklist pending_review_worklist.json
```

The verifier independently rebuilds the source selection and two-reviewer
assignment schedule. It checks the complete canonical worklist, including
source revisions, reviewer slots, all input-byte SHA-256 receipts, assignment
policy, and explicit pending states. Duplicate JSON keys, extra fields,
altered reviewers or source identities, and changed input bytes fail closed.
Tests cover valid replay and tampered assignments, identities, statuses and
input bytes.

This proves local deterministic assignment consistency only. Reviewer IDs
are operator supplied; it does not authenticate Wikimedia provenance,
preregistration timing, reviewer independence, judgments, or entailment.
