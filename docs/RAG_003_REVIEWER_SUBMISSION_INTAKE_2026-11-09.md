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


## Opt-in strict local evidence-byte checks

Provide `--evidence-root` and `--artifact-map` together to
require every submitted source SHA-256 and answer SHA-256 to match the **actual**
nonempty regular files inside the selected local directory, without following
symbolic links or `..` path components. Each source revision and each
reviewed case must have exactly one independently stored file. The verifier
rejects hard-linked aliases, changed files during reads, missing or extra map
entries, oversized files, and invalid UTF-8/empty/nonlexical claim byte ranges.

The JSON map uses the following schema:

```json
{
  "schema_version": 1,
  "sources": [
    {"project":"enwiki","page_id":123,"revision_id":456,"path":"sources/123.txt"}
  ],
  "answers": [
    {"case_id":"case_1","path":"answers/case_1.txt"}
  ]
}
```

Example strict command (append to the structural invocation above):

```sh
  --evidence-root path/to/local-evidence \
  --artifact-map artifact-map.json
```

The private receipt distinguishes the stronger local checks by setting
`source_artifact_hashes_verified`, `answer_artifact_hashes_verified`
and `claim_byte_spans_verified` to true, and pins the map's exact
input-byte SHA-256. **This means local byte/digest consistency only.** It
does not prove the bytes came from Wikimedia or the model, that the claimed
source text entails the answer, that the reviewer exists, or that human
judgment was independent. Both modes retain `qualification_status:
"not_qualified"`; the existing authenticated source, reviewer and
human evaluation gates remain mandatory.

### Exact source quotation and disjoint claim checks

Strict local evidence mode also requires each reviewer-supplied `source_span` to
appear **verbatim as UTF-8 bytes** in the corresponding locally verified source
artifact. This is a literal-presence check only, **not** entailment, citation
correctness, or proof of publisher authenticity. Submissions with overlapping
claim byte intervals for the same case are rejected in both modes to prevent
double-counting partially duplicated claim reviews. Adjacent intervals remain
valid. Non-text source artifacts are not supported by this strict quotation gate.

The structural overlap check sorts claim intervals per case and rejects intersecting half-open ranges in O(n log n) time rather than scanning every previous claim. This keeps large valid reviewer submissions within the declared 50,000-judgment budget; adjacent intervals are permitted.

### Reviewer quotation and rubric text safety

Structural intake now requires `source_span` (at most 4,096 **UTF-8 bytes**)
and `rubric_version` (at most 64 UTF-8 bytes) to contain visible lexical
characters and no leading/trailing whitespace, control characters, Unicode
format controls (including bidi overrides and zero-width marks), or surrogate
code points. Ordinary multilingual quotations and rubric names remain valid.
These checks reduce invisible/spoofed evidence labels; they do **not** establish
quotation truth or reviewer authenticity. RAG-003 remains incomplete.

### Review timestamp sanity

Structural intake rejects reviewer `reviewed_at` timestamps more than five
minutes ahead of the verifier's current UTC clock, including timestamps with
non-UTC offsets. This tolerates minor clock skew but prevents a private receipt
from treating a future event as already performed. Calendar-invalid dates also
fail. This is **not** a trusted timestamp, proof of human presence, or evidence
that a claimed review occurred; reviewer authentication remains external.

### Bounded-memory source quotation verification

The strict verifier streams source revision files in 64 KiB blocks and checks
all submitted literal quotations across block boundaries while hashing the
same opened file descriptor. It does not retain entire source artifacts in
memory (which may be up to 256 MiB each). Answer artifacts remain bounded to
16 MiB per case for UTF-8 claim byte slicing. This reduces source-file
buffering but does not establish publisher authenticity or answer entailment.
