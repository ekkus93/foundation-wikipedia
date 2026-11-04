#!/usr/bin/env python3
"""Validate the RAG-003 reviewed source set against deterministic sampling."""
import argparse
import json
import sys

from sample_rag003_articles import (MAX_INVENTORY_BYTES, MAX_PLAN_BYTES,
                                    build_roster, read_pinned_input)
from validate_rag003_review_manifest import (MAX_MANIFEST_BYTES, validate_manifest,
                                            validate_structure)
from verify_rag003_sample_roster import MAX_RECEIPT_BYTES


def validate_review_selection(manifest, plan, replayed):
    """Check exact source revisions and plan metadata; not publisher authenticity."""
    problems = []
    recorded = manifest["preregistration"]
    for field in ("plan_sha256", "sampling_seed", "minimum_articles_per_project"):
        expected = replayed[field]
        if type(recorded.get(field)) is not type(expected) or recorded[field] != expected:
            problems.append(f"preregistration.{field}: does not match replayed sampling plan")
    for field, roster_field in (("required_projects", "required_projects"),
                                ("strata", "required_strata")):
        declared = recorded[field]
        if len(declared) != len(set(declared)) or set(declared) != set(replayed[roster_field]):
            problems.append(f"preregistration.{field}: does not match replayed sampling plan")
    selected = {(row["project"], row["page_id"], row["revision_id"])
                for row in replayed["selected"]}
    actual = {(row["project"], row["page_id"], row["revision_id"])
              for row in manifest["source_articles"]}
    if selected - actual:
        problems.append(f"source_articles: {len(selected - actual)} selected revision(s) missing")
    if actual - selected:
        problems.append(f"source_articles: {len(actual - selected)} unselected revision(s) present")
    if len(manifest["source_articles"]) != len(selected):
        problems.append("source_articles: expected exactly one record per selected revision")
    return problems


def verify_review_selection(manifest_path, plan_path, inventory_path, receipt_path,
                            evidence_root=None):
    """Replay from single bounded input reads and bind the complete review manifest."""
    manifest, _ = read_pinned_input(manifest_path, MAX_MANIFEST_BYTES)
    # Structural failures can make downstream fields unsafe to access. For a
    # structurally valid manifest, collect semantic failures AND selection
    # mismatches; neither one is allowed to conceal the other in diagnostics.
    structural = validate_structure(manifest)
    if structural:
        return structural
    problems = validate_manifest(manifest, evidence_root=evidence_root)
    plan, plan_hash = read_pinned_input(plan_path, MAX_PLAN_BYTES)
    inventory, inventory_hash = read_pinned_input(inventory_path, MAX_INVENTORY_BYTES)
    receipt, _ = read_pinned_input(receipt_path, MAX_RECEIPT_BYTES)
    replayed = build_roster(plan, inventory)
    replayed["plan_sha256"] = plan_hash
    replayed["inventory_sha256"] = inventory_hash
    # Equality of decoded JSON alone treats booleans as integers in Python.
    canonical = lambda value: json.dumps(value, sort_keys=True, separators=(",", ":"),
                                        ensure_ascii=False)
    if not isinstance(receipt, dict) or canonical(receipt) != canonical(replayed):
        raise ValueError("sampling receipt does not match deterministic replay and exact input hashes")
    return problems + validate_review_selection(manifest, plan, replayed)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", required=True)
    parser.add_argument("--plan", required=True)
    parser.add_argument("--inventory", required=True)
    parser.add_argument("--receipt", required=True)
    parser.add_argument("--evidence-root", metavar="DIR",
                        help="Also verify required source and evaluation files by SHA-256")
    args = parser.parse_args(argv)
    try:
        problems = verify_review_selection(args.manifest, args.plan, args.inventory,
                                           args.receipt, evidence_root=args.evidence_root)
    except (OSError, ValueError, TypeError, UnicodeError, RuntimeError) as exc:
        problems = [str(exc)]
    for error in problems:
        print(error, file=sys.stderr)
    if problems:
        return 1
    print("Reviewed source revisions match deterministic sampling; "
          "Wikimedia and reviewer authenticity NOT established")
    return 0


if __name__ == "__main__":
    sys.exit(main())
