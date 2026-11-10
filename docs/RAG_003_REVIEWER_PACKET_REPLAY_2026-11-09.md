# RAG-003 — Reviewer packet replay verification

**Status:** Partial; RAG-003 remains unchecked.

After creating a private pending packet with `export_rag003_reviewer_packet.py`,
verify it using the exact original pinned inputs:

```sh
python3 scripts/verify_rag003_reviewer_packet.py \
  --plan pinned_sampling_plan.json \
  --inventory pinned_candidate_inventory.json \
  --receipt selected_roster.json \
  --reviewers reviewer_ids.json \
  --worklist pending_review_worklist.json \
  --packet private_reviewer_packet.json
```

The verifier independently replays the complete worklist and recomputes the
reviewer-scoped packet. It rejects changed source revisions, added fields,
peer reviewer identities, altered status claims, unknown reviewers and
mismatched input bytes. The packet itself remains pending-only: this is
neither publisher-authenticated source acquisition nor evidence of independent
human judgment, factual entailment, or completed RAG-003 acceptance.
