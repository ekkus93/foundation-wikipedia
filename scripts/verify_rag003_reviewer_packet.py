#!/usr/bin/env python3
"""Verify a private reviewer packet by replaying all pinned RAG-003 inputs."""
import argparse
import json
import sys
from pathlib import Path

from export_rag003_reviewer_packet import build_packet
from sample_rag003_articles import read_pinned_input
from verify_rag003_review_worklist import MAX_WORKLIST_BYTES, verify

MAX_PACKET_BYTES = 16 * 1024 * 1024


def verify_packet(plan, inventory, receipt, reviewers, worklist, packet_path):
    # Replay the complete worklist, not merely the hashes inside the packet.
    verify(plan, inventory, receipt, reviewers, worklist)
    queue, queue_hash = read_pinned_input(worklist, MAX_WORKLIST_BYTES)
    packet, _ = read_pinned_input(packet_path, MAX_PACKET_BYTES)
    if not isinstance(packet, dict) or type(packet.get("reviewer_id")) is not str:
        raise ValueError("invalid packet reviewer identity")
    expected = build_packet(queue, queue_hash, packet["reviewer_id"])
    canonical = lambda value: json.dumps(value, sort_keys=True, ensure_ascii=False,
                                         allow_nan=False, separators=(",", ":"))
    if canonical(packet) != canonical(expected):
        raise ValueError("review packet differs from pinned reviewer-specific replay")
    return expected["assignment_count"]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("plan", "inventory", "receipt", "reviewers", "worklist", "packet"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        count = verify_packet(args.plan, args.inventory, args.receipt, args.reviewers,
                              args.worklist, args.packet)
        print(f"Verified {count} pending reviewer assignments; no human review certified")
        return 0
    except (OSError, ValueError, TypeError, UnicodeError, OverflowError, KeyError) as exc:
        print(f"review packet verification failed: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
