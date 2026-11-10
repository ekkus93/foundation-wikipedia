#!/usr/bin/env python3
"""Export one reviewer's pending source assignments without exposing peer identities."""
import argparse
import hashlib
import json
from rag003_private_output import publish_private_bytes
import sys
from pathlib import Path

from sample_rag003_articles import read_pinned_input
from verify_rag003_review_worklist import MAX_WORKLIST_BYTES, verify


def build_packet(worklist, worklist_sha256, reviewer_id):
    if reviewer_id not in worklist["reviewer_ids"]:
        raise ValueError("reviewer not present in pinned roster")
    selected = []
    for row in worklist["assignments"]:
        if reviewer_id in row["assigned_reviewer_ids"]:
            selected.append({
                "project": row["project"], "page_id": row["page_id"],
                "revision_id": row["revision_id"], "strata": row["strata"],
                "source_acquisition": "pending", "independent_review": "pending",
            })
    return {
        "schema_version": 1, "packet_policy": "private-pending-reviewer-v1",
        "worklist_sha256": worklist_sha256, "reviewer_id": reviewer_id,
        "assignment_count": len(selected), "assignments": selected,
        "warning": "Scheduling only; no authenticated sources or completed human judgments",
    }


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("plan", "inventory", "receipt", "reviewers", "worklist"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--reviewer-id", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        verify(args.plan, args.inventory, args.receipt, args.reviewers, args.worklist)
        worklist, worklist_hash = read_pinned_input(args.worklist, MAX_WORKLIST_BYTES)
        packet = build_packet(worklist, worklist_hash, args.reviewer_id)
        payload = (json.dumps(packet, sort_keys=True, ensure_ascii=False,
                              allow_nan=False, separators=(",", ":")) + "\n").encode("utf-8")
        publish_private_bytes(args.output, payload)
        print(f'Prepared {packet["assignment_count"]} pending assignments for reviewer')
        return 0
    except (OSError, ValueError, TypeError, UnicodeError, OverflowError, KeyError) as exc:
        print(f"review packet export failed: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
