# RAG-003 — Structural intake for reviewer-authored judgments

**Status:** Partial. RAG-003 remains unchecked. This verifier does **not**
authenticate Wikimedia publications, the reviewer, independently performed
human review, answer correctness, or source bytes.

The pending-worklist and private-packet exporters schedule two reviewers per
sampled article but cannot collect or certify judgments. After separately
acquiring actual revision-matched source files and reviewing generated answers,
the reviewer may prepare a JSON submission:

```json
{
  "schema_version": 1,
  "submission_policy": "reviewer-claims-v1",
  "packet_sha256": "<64 lowercase hex SHA-256 of exact private packet bytes>",
  "reviewer_id": "reviewer_a",
  "judgments": [{
    "case_id": "case_1",
    "project": "enwiki",
    "page_id": 123,
    "revision_id": 456,
    "answer_sha256": "<64 lowercase hex>",
    "source_artifact_sha256": "<64 lowercase hex>",
    "claim_start_byte": 0,
    "claim_end_byte": 12,
    "reviewed_at": "2026-11-10T06:00:00Z",
    "rubric_version": "v1",
    "verdict": "supported",
    "source_span": "Literal passage reviewed by the person",
    "citation_valid": true
  }]
}
```

Use actual IDs/revisions from the reviewer's verified private packet. Byte spans
refer to the exact generated-answer bytes whose SHA-256 the reviewer records.
Reviews can be partial: a successful structural receipt counts assigned source
revisions with submitted judgments, not completed or authenticated reviews.

```sh
python3 scripts/verify_rag003_reviewer_submission.py \
  --plan pinned_sampling_plan.json \
  --inventory pinned_candidate_inventory.json \
  --receipt selected_roster.json \
  --reviewers reviewer_ids.json \
  --worklist pending_review_worklist.json \
  --packet private_reviewer_packet.json \
  --submission reviewer_authored_judgments.json \
  --report private_structural_receipt.json
```

The validator replays the original sampling and complete worklist, then the
reviewer-scoped packet, and rejects changed input bytes, unassigned revisions,
unknown reviewers, duplicate JSON keys, unrecognized status assertions,
conflicting source/answer hashes, reused case identities, duplicate claim
spans, malformed timestamps, and invalid verdicts. The optional receipt is
exclusively published with private 0600 POSIX permissions and never overwrites
an existing path. A valid submission is **not** imported into the qualified
manifest automatically. A separate authenticated source-artifact verification,
actual reviewer authentication/independence checks, reviewer evidence review,
and the preexisting RAG-003 manifest/answer-score gates remain mandatory.
