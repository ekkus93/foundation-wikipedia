# RAG-003 — Reviewer-scoped pending assignment packets

**Status:** Partial; RAG-003 remains unchecked.

After generating the pinned selection receipt and review worklist, export a
reviewer-specific packet with:

```sh
python3 scripts/export_rag003_reviewer_packet.py \
  --plan pinned_sampling_plan.json \
  --inventory pinned_candidate_inventory.json \
  --receipt selected_roster.json \
  --reviewers reviewer_ids.json \
  --worklist pending_review_worklist.json \
  --reviewer-id reviewer_a \
  --output reviewer_a_pending.json
```

The exporter first independently replays the **entire** pinned worklist and
all input byte hashes. It rejects altered worklists, absent reviewer IDs and
existing output files. Only the chosen reviewer's selected source identities,
strata and explicit pending statuses appear in the packet. Peer reviewer
identities and assignments are not disclosed. The packet pins the SHA-256 of
the exact original worklist bytes, enabling later provenance reconciliation.

**Limitations:** This is an assignment packet, not a reviewer submission or
authenticated Wikimedia acquisition. The reviewer ID is not an authenticated
identity, and distributing separate packets does not prove independence.
No review judgments, factual entailment, publisher provenance, preregistration
timing or physical-device acceptance are asserted.

**Private local publication (POSIX):** Reviewer packets and the complete
reviewer-assignment worklist use a same-directory 0600 temporary file, fsync
its bytes, then atomically hard-link to a previously absent destination.
A permissive process umask does not expose reviewer identities or assignments.
Existing files and symlinks are never overwritten, and failed publication
removes the private temporary file. Restrict access to the containing directory
and distribute reviewer packets through an authenticated, access-controlled
channel. This does not authenticate the reviewer or guarantee power-loss durability.
