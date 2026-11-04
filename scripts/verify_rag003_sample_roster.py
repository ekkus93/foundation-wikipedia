#!/usr/bin/env python3
"""Replay a RAG-003 sampling receipt against the exact pinned input bytes."""
import argparse
import json
import sys
from pathlib import Path

from sample_rag003_articles import (MAX_INVENTORY_BYTES, MAX_PLAN_BYTES,
                                    build_roster, read_pinned_input)

MAX_RECEIPT_BYTES = 32 * 1024 * 1024


def verify(plan_path, inventory_path, receipt_path):
    plan, plan_hash = read_pinned_input(plan_path, MAX_PLAN_BYTES)
    inventory, inventory_hash = read_pinned_input(inventory_path, MAX_INVENTORY_BYTES)
    observed, _ = read_pinned_input(receipt_path, MAX_RECEIPT_BYTES)
    expected = build_roster(plan, inventory)
    expected["plan_sha256"] = plan_hash
    expected["inventory_sha256"] = inventory_hash
    if not isinstance(observed, dict) or observed != expected:
        raise ValueError("sampling receipt does not match pinned inputs and deterministic replay")
    # Python considers True == 1, so also require canonical serialization to
    # reject booleans masquerading as integers or noncanonical extra structure.
    if json.dumps(observed, sort_keys=True, separators=(",", ":"), ensure_ascii=False) != json.dumps(
            expected, sort_keys=True, separators=(",", ":"), ensure_ascii=False):
        raise ValueError("sampling receipt has noncanonical values")
    return len(expected["selected"])


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--inventory", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        count = verify(args.plan, args.inventory, args.receipt)
        print(f"Verified deterministic replay of {count} source revisions; "
              "this does not authenticate Wikimedia or preregistration timing")
        return 0
    except (OSError, ValueError, TypeError, UnicodeError) as exc:
        print(f"sampling receipt verification failed: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
