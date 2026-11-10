#!/usr/bin/env python3
"""Validate reviewer-authored RAG-003 judgments against a pinned private packet.

This validates submission structure and local assignment provenance, NOT
reviewer identity, source authenticity, answer entailment, or human independence.
"""
import argparse
from datetime import datetime
import json
from pathlib import Path
import re
import sys

from rag003_private_output import publish_private_bytes
from sample_rag003_articles import read_pinned_input
from verify_rag003_reviewer_packet import MAX_PACKET_BYTES, verify_packet

MAX_SUBMISSION_BYTES = 16 * 1024 * 1024
MAX_JUDGMENTS = 50_000
SHA256 = re.compile(r"[0-9a-f]{64}\Z")
CASE_ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}\Z")
VERDICTS = {"supported", "contradicted", "not_established", "unreviewable"}
JUDGMENT_FIELDS = {
    "case_id", "project", "page_id", "revision_id", "answer_sha256",
    "source_artifact_sha256", "claim_start_byte", "claim_end_byte",
    "reviewed_at", "rubric_version", "verdict", "source_span", "citation_valid",
}


def _timestamp(value):
    if not isinstance(value, str) or not 0 < len(value) <= 64:
        raise ValueError("reviewed_at must be a bounded timezone-aware timestamp")
    try:
        result = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as exc:
        raise ValueError("invalid reviewed_at timestamp") from exc
    if result.utcoffset() is None:
        raise ValueError("reviewed_at timestamp lacks timezone")


def _digest(value):
    return isinstance(value, str) and SHA256.fullmatch(value) is not None


def validate_submission(packet, packet_sha256, submission, submission_sha256):
    required = {"schema_version", "submission_policy", "packet_sha256",
                "reviewer_id", "judgments"}
    if not isinstance(submission, dict) or set(submission) != required:
        raise ValueError("submission has missing or unexpected fields")
    if type(submission["schema_version"]) is not int or submission["schema_version"] != 1:
        raise ValueError("unsupported submission schema version")
    if submission["submission_policy"] != "reviewer-claims-v1":
        raise ValueError("unsupported submission policy")
    if submission["reviewer_id"] != packet["reviewer_id"]:
        raise ValueError("reviewer does not match pinned private packet")
    if submission["packet_sha256"] != packet_sha256:
        raise ValueError("submission packet SHA-256 does not match pinned bytes")
    records = submission["judgments"]
    if not isinstance(records, list) or not 0 < len(records) <= MAX_JUDGMENTS:
        raise ValueError("require 1-50,000 explicit reviewer judgments")
    assigned = {(row["project"], row["page_id"], row["revision_id"])
                for row in packet["assignments"]}
    seen_claims, cases, sources = set(), {}, {}
    covered = set()
    for index, item in enumerate(records):
        prefix = f"judgments[{index}]"
        if not isinstance(item, dict) or set(item) != JUDGMENT_FIELDS:
            raise ValueError(f"{prefix}: missing or unexpected fields")
        identity = (item["project"], item["page_id"], item["revision_id"])
        if (type(item["project"]) is not str or
                type(item["page_id"]) is not int or
                type(item["revision_id"]) is not int or identity not in assigned):
            raise ValueError(f"{prefix}: source revision not assigned to reviewer")
        if not isinstance(item["case_id"], str) or not CASE_ID.fullmatch(item["case_id"]):
            raise ValueError(f"{prefix}: invalid case ID")
        if not _digest(item["answer_sha256"]) or not _digest(item["source_artifact_sha256"]):
            raise ValueError(f"{prefix}: invalid answer/source artifact hash")
        start, end = item["claim_start_byte"], item["claim_end_byte"]
        if (type(start) is not int or type(end) is not int or
                not 0 <= start < end <= 16 * 1024 * 1024):
            raise ValueError(f"{prefix}: invalid claim byte span")
        if (item["verdict"] not in VERDICTS or
                type(item["citation_valid"]) is not bool):
            raise ValueError(f"{prefix}: invalid verdict or citation-validity flag")
        if (not isinstance(item["rubric_version"], str) or
                not 0 < len(item["rubric_version"]) <= 64 or
                not item["rubric_version"].strip() == item["rubric_version"] or
                not isinstance(item["source_span"], str) or
                not 0 < len(item["source_span"]) <= 4096 or
                not item["source_span"].strip()):
            raise ValueError(f"{prefix}: invalid review rubric or source span")
        _timestamp(item["reviewed_at"])
        case_key = (identity, item["answer_sha256"], item["rubric_version"])
        prior = cases.setdefault(item["case_id"], case_key)
        if prior != case_key:
            raise ValueError(f"{prefix}: case ID reused for different source/answer/rubric")
        previous_source = sources.setdefault(identity, item["source_artifact_sha256"])
        if previous_source != item["source_artifact_sha256"]:
            raise ValueError(f"{prefix}: conflicting source artifact hashes")
        claim_key = (item["case_id"], start, end)
        if claim_key in seen_claims:
            raise ValueError(f"{prefix}: duplicate reviewed claim span")
        seen_claims.add(claim_key)
        covered.add(identity)
    return {
        "schema_version": 1,
        "verification_scope": "structure-and-pinned-assignment-only",
        "packet_sha256": packet_sha256,
        "submission_sha256": submission_sha256,
        "reviewer_id": packet["reviewer_id"],
        "judgment_count": len(records),
        "assigned_source_count": len(assigned),
        "sources_with_submitted_judgments": len(covered),
        "reviewer_identity_authenticated": False,
        "source_artifact_hashes_verified": False,
        "answer_claims_factually_verified": False,
        "independent_human_review_certified": False,
        "qualification_status": "not_qualified",
    }


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("plan", "inventory", "receipt", "reviewers",
                 "worklist", "packet", "submission"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--report", type=Path,
                        help="Optional exclusive private structural receipt (not qualification)")
    args = parser.parse_args(argv)
    try:
        verify_packet(args.plan, args.inventory, args.receipt, args.reviewers,
                      args.worklist, args.packet)
        packet, packet_hash = read_pinned_input(args.packet, MAX_PACKET_BYTES)
        submission, submission_hash = read_pinned_input(args.submission, MAX_SUBMISSION_BYTES)
        result = validate_submission(packet, packet_hash, submission, submission_hash)
        if args.report is not None:
            payload = (json.dumps(result, sort_keys=True, separators=(",", ":"),
                                  ensure_ascii=False, allow_nan=False) + "\n").encode("utf-8")
            publish_private_bytes(args.report, payload)
        print(f'Validated {result["judgment_count"]} structurally bound claims '
              f'across {result["sources_with_submitted_judgments"]}/'
              f'{result["assigned_source_count"]} assigned sources; '
              'no human attribution, source authentication or entailment certified')
        return 0
    except (OSError, ValueError, TypeError, KeyError, OverflowError, UnicodeError) as exc:
        print(f"reviewer submission validation failed: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
