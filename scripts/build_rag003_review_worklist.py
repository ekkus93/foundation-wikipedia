#!/usr/bin/env python3
"""Prepare deterministic two-reviewer assignments; never manufacture review evidence."""
import argparse
import hashlib
import json
from rag003_private_output import publish_private_bytes
import re
import sys
from pathlib import Path

from sample_rag003_articles import (MAX_INVENTORY_BYTES, MAX_PLAN_BYTES,
                                    build_roster, read_pinned_input)
from verify_rag003_sample_roster import MAX_RECEIPT_BYTES

MAX_REVIEWER_BYTES = 16 * 1024
MAX_ASSIGNED_ARTICLES = 10_000
REVIEWER_ID = re.compile(r"[A-Za-z][A-Za-z0-9_-]{1,63}\Z")


def build_worklist(plan, inventory, receipt, reviewers, plan_hash,
                   inventory_hash, receipt_hash, reviewers_hash):
    """Replay source selection before scheduling two independent review slots."""
    replay = build_roster(plan, inventory)
    replay["plan_sha256"] = plan_hash
    replay["inventory_sha256"] = inventory_hash
    canonical = lambda value: json.dumps(value, sort_keys=True, separators=(",", ":"),
                                         ensure_ascii=False, allow_nan=False)
    if not isinstance(receipt, dict) or canonical(receipt) != canonical(replay):
        raise ValueError("sampling receipt does not match pinned input bytes and replay")
    if not isinstance(reviewers, dict) or set(reviewers) != {"reviewer_ids"}:
        raise ValueError("reviewers must contain exactly reviewer_ids")
    ids = reviewers["reviewer_ids"]
    if (not isinstance(ids, list) or not 2 <= len(ids) <= 32
            or not all(isinstance(value, str) and REVIEWER_ID.fullmatch(value) for value in ids)
            or len(ids) != len(set(ids))):
        raise ValueError("require 2-32 distinct valid reviewer IDs")
    if len(replay["selected"]) > MAX_ASSIGNED_ARTICLES:
        raise ValueError("review worklist exceeds 10,000 selected articles")
    loads = {project: {reviewer: 0 for reviewer in ids}
             for project in replay["required_projects"]}
    assignments = []
    # Stable source identity ordering is independent of source inventory ordering.
    for source in sorted(replay["selected"],
                         key=lambda item: (item["project"], item["page_id"],
                                           item["revision_id"])):
        project = source["project"]
        identity = f'{project}:{source["page_id"]}:{source["revision_id"]}'
        def rank(reviewer):
            digest = hashlib.sha256(f'{receipt_hash}:{identity}:{reviewer}'.encode("ascii")).digest()
            return loads[project][reviewer], digest, reviewer
        selected = sorted(ids, key=rank)[:2]
        for reviewer in selected:
            loads[project][reviewer] += 1
        assignments.append({
            "project": project, "page_id": source["page_id"],
            "revision_id": source["revision_id"], "strata": source["strata"],
            "assigned_reviewer_ids": selected,
            "source_acquisition": "pending", "independent_reviews": "pending",
        })
    return {
        "schema_version": 1, "assignment_policy": "balanced-two-reviewers-sha256-v1",
        "plan_sha256": plan_hash, "inventory_sha256": inventory_hash,
        "sampling_receipt_sha256": receipt_hash, "reviewer_roster_sha256": reviewers_hash,
        "reviewer_ids": sorted(ids),
        "source_count": len(assignments), "review_slots": len(assignments) * 2,
        "assignments": assignments,
        "warning": "Assignment only: no authenticated source acquisition, independent reviews or factual entailment",
    }


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--plan", required=True, type=Path)
    parser.add_argument("--inventory", required=True, type=Path)
    parser.add_argument("--receipt", required=True, type=Path)
    parser.add_argument("--reviewers", required=True, type=Path,
                        help='JSON {"reviewer_ids":["reviewer_a","reviewer_b"]}')
    parser.add_argument("--output", required=True, type=Path,
                        help="Create a new worklist; never overwrite existing evidence")
    args = parser.parse_args(argv)
    try:
        plan, plan_hash = read_pinned_input(args.plan, MAX_PLAN_BYTES)
        inventory, inventory_hash = read_pinned_input(args.inventory, MAX_INVENTORY_BYTES)
        receipt, receipt_hash = read_pinned_input(args.receipt, MAX_RECEIPT_BYTES)
        reviewers, reviewers_hash = read_pinned_input(args.reviewers, MAX_REVIEWER_BYTES)
        queue = build_worklist(plan, inventory, receipt, reviewers, plan_hash,
                               inventory_hash, receipt_hash, reviewers_hash)
        payload = (json.dumps(queue, sort_keys=True, separators=(",", ":"),
                              ensure_ascii=False, allow_nan=False) + "\n").encode("utf-8")
        publish_private_bytes(args.output, payload)
        print(f'Prepared {queue["source_count"]} source records and '
              f'{queue["review_slots"]} pending review slots; no judgments created')
        return 0
    except (OSError, ValueError, TypeError, UnicodeError, OverflowError) as exc:
        print(f"review assignment failed: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
