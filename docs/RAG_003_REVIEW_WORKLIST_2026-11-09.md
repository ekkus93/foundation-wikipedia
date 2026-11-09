# RAG-003 — Pinned independent-review assignment worklist

**Status:** Partial implementation. RAG-003 remains unchecked.

Use this after producing a deterministic source-revision sampling receipt:

\`\`\`sh
python3 scripts/build_rag003_review_worklist.py \
  --plan pinned_sampling_plan.json \
  --inventory pinned_candidate_inventory.json \
  --receipt selected_roster.json \
  --reviewers reviewer_ids.json \
  --output pending_review_worklist.json
\`\`\`

The reviewer file contains \`{"reviewer_ids":["reviewer_a","reviewer_b","reviewer_c"]}\`.
The script independently replays the selected project/page/revision roster
against the exact pinned plan/inventory bytes and sampling receipt. It rejects
substituted source revisions or altered receipts, requires two to 32 distinct
well-formed reviewer identifiers, and assigns two different reviewers to each
source with balanced **per-project** review loads. SHA-256 breaks load ties
deterministically; no random generator or external service is needed.
Outputs are exclusive-create and never overwrite prior results.

Every entry explicitly sets both source acquisition and independent reviews to
\`pending\`. Review assignments must be distributed separately to actual
independent human reviewers; a schedule does not prove independence. Reviewer
judgments, acquired revision bytes, publisher provenance, preregistration
timing and factual-entailment outcomes are not fabricated. Qualifying results
must still go through the full reviewed-manifest schema and
\`verify_rag003_review_selection.py\` gate before RAG-003 completion.
