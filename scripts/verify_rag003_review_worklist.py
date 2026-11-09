#!/usr/bin/env python3
"""Independently replay a pending RAG-003 review worklist against pinned inputs."""
import argparse
import json
import sys
from pathlib import Path

from build_rag003_review_worklist import (MAX_REVIEWER_BYTES, build_worklist)
from sample_rag003_articles import MAX_INVENTORY_BYTES, MAX_PLAN_BYTES, read_pinned_input
from verify_rag003_sample_roster import MAX_RECEIPT_BYTES

MAX_WORKLIST_BYTES = 32 * 1024 * 1024


def verify(plan_path, inventory_path, receipt_path, reviewers_path, worklist_path):
    plan, plan_hash = read_pinned_input(plan_path, MAX_PLAN_BYTES)
    inventory, inventory_hash = read_pinned_input(inventory_path, MAX_INVENTORY_BYTES)
    receipt, receipt_hash = read_pinned_input(receipt_path, MAX_RECEIPT_BYTES)
    reviewers, reviewers_hash = read_pinned_input(reviewers_path, MAX_REVIEWER_BYTES)
    observed, _ = read_pinned_input(worklist_path, MAX_WORKLIST_BYTES)
    expected = build_worklist(plan, inventory, receipt, reviewers, plan_hash,
                              inventory_hash, receipt_hash, reviewers_hash)
    # Canonical JSON comparison distinguishes booleans from integers, rejects
    # added fields and ensures every assigned source/reviewer/status is replayed.
    canonical = lambda item: json.dumps(item, sort_keys=True, separators=(",", ":"),
                                        ensure_ascii=False, allow_nan=False)
    if not isinstance(observed, dict) or canonical(observed) != canonical(expected):
        raise ValueError("review worklist differs from pinned deterministic replay")
    return expected["source_count"]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("plan", "inventory", "receipt", "reviewers", "worklist"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        count = verify(args.plan, args.inventory, args.receipt, args.reviewers,
                       args.worklist)
        print(f"Verified {count} pending source assignments; no independent review is certified")
        return 0
    except (OSError, ValueError, TypeError, UnicodeError, OverflowError) as exc:
        print(f"review worklist verification failed: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
