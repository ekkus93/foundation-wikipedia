#!/usr/bin/env python3
"""RAG-003 semantic manifest checks; not a substitute for JSON Schema or authentication."""
import argparse
import datetime
import json
import re
import sys
from pathlib import Path

MAX_MANIFEST_BYTES = 2 * 1024 * 1024
SCHEMA_PATH = Path(__file__).resolve().parent.parent / 'docs' / 'schemas' / 'rag003-review-manifest.schema.json'

def timestamp(value):
    parsed = datetime.datetime.fromisoformat(value.replace("Z", "+00:00"))
    if parsed.utcoffset() is None:
        raise ValueError("timestamp missing timezone")
    return parsed

def validate(data):
    errors = []
    try:
        plan, sources, reviews, run = (data[k] for k in ("preregistration", "source_articles", "reviews", "execution"))
        registered = timestamp(plan["registered_at"])
        required = plan["required_projects"]
        if not required or len(required) != len(set(required)):
            errors.append("required_projects must be nonempty and unique")
        identities, acquisition_times, reviewed_articles, cases, judgments = set(), {}, {}, {}, set()
        duplicate_sources = set()
        qualifying_reviewers = {}
        case_projects = {}
        case_sources = {}
        case_claim_spans = {}
        case_rubrics = {}
        for i, source in enumerate(sources):
            identity = (source["project"], source["page_id"], source["revision_id"])
            if identity in identities:
                errors.append(f"source_articles[{i}]: duplicate revision")
                duplicate_sources.add(identity)
            identities.add(identity)
            if source["project"] not in required:
                errors.append(f"source_articles[{i}]: project outside preregistered required_projects")
            acquired = timestamp(source["acquired_at"])
            acquisition_times.setdefault(identity, acquired)
            if acquired < registered:
                errors.append(f"source_articles[{i}]: acquisition predates registration")
        for i, review in enumerate(reviews):
            identity = (review["source_project"], review["source_page_id"], review["source_revision_id"])
            if identity not in identities:
                errors.append(f"reviews[{i}]: unknown source revision")
            else:
                previous_source = case_sources.setdefault(review["case_id"], identity)
                if previous_source != identity:
                    errors.append(f"reviews[{i}]: case_id reused across source revisions")
                previous_project = case_projects.setdefault(review["case_id"], identity[0])
                if previous_project != identity[0]:
                    errors.append(f"reviews[{i}]: case_id reused across projects")
            claim_span = (review["claim_start_byte"], review["claim_end_byte"])
            previous_span = case_claim_spans.setdefault(review["case_id"], claim_span)
            conflicting_span = previous_span != claim_span
            if conflicting_span:
                errors.append(f"reviews[{i}]: case_id reused with conflicting claim span")
            rubric = review["rubric_version"]
            previous_rubric = case_rubrics.setdefault(review["case_id"], rubric)
            conflicting_rubric = previous_rubric != rubric
            if conflicting_rubric:
                errors.append(f"reviews[{i}]: case_id reused with conflicting rubric version")
            key = (review["case_id"], review["reviewer_id"], review["claim_start_byte"], review["claim_end_byte"])
            duplicate_judgment = key in judgments
            if duplicate_judgment:
                errors.append(f"reviews[{i}]: duplicate reviewer judgment")
            judgments.add(key)
            if review["claim_start_byte"] < 0 or review["claim_end_byte"] <= review["claim_start_byte"]:
                errors.append(f"reviews[{i}]: invalid claim byte span")
            reviewed = timestamp(review["reviewed_at"])
            if reviewed < registered:
                errors.append(f"reviews[{i}]: review predates registration")
            if identity in acquisition_times and reviewed < acquisition_times[identity]:
                errors.append(f"reviews[{i}]: review predates source acquisition")
            if (not duplicate_judgment and not conflicting_span and not conflicting_rubric
                    and identity not in duplicate_sources and identity in acquisition_times and reviewed >= registered
                    and reviewed >= acquisition_times[identity]
                    and review["claim_start_byte"] >= 0
                    and review["claim_end_byte"] > review["claim_start_byte"]
                    and case_projects.get(review["case_id"]) == identity[0]
                    and case_sources.get(review["case_id"]) == identity):
                qualifying_reviewers.setdefault(review["case_id"], set()).add(review["reviewer_id"])
        # A single review (or a replayed reviewer's judgment) cannot qualify a
        # sampled case. Only distinct, individually valid reviewer IDs count;
        # this is a minimum evidence check, not proof of human independence.
        for case_id, source_identity in case_sources.items():
            reviewers = qualifying_reviewers.get(case_id, set())
            if len(reviewers) < 2:
                errors.append(f"{case_id}: fewer than two distinct valid reviewers")
                continue
            cases.setdefault(source_identity[0], set()).add(case_id)
            reviewed_articles.setdefault(source_identity[0], set()).add(source_identity[1])
        # A valid sampling plan cannot silently shrink a declared project to
        # zero source articles or qualify a case from an unplanned project.
        for project in required:
            if not any(identity[0] == project for identity in identities):
                errors.append(f"{project}: no acquired source articles")
            if len(reviewed_articles.get(project, set())) < plan["minimum_articles_per_project"]:
                errors.append(f"{project}: insufficient distinct articles")
            if len(cases.get(project, set())) < plan["minimum_cases_per_project"]:
                errors.append(f"{project}: insufficient reviewed cases")
        if not re.fullmatch(r"[0-9a-f]{40}", run["runner_commit"]):
            errors.append("invalid runner commit")
        for key in ("suite_sha256", "corpus_sha256", "report_sha256", "model_config_sha256"):
            if not re.fullmatch(r"[0-9a-f]{64}", run[key]):
                errors.append(f"invalid {key}")
    except (KeyError, TypeError, ValueError, AttributeError) as exc:
        errors.append(f"invalid manifest: {exc}")
    return errors


def validate_structure(data):
    """Assert checked-in Draft 2020-12 rules, including date-time/URI formats.

    This checks structure, not source authenticity or review independence.
    Missing jsonschema is a fatal configuration error, never a skipped gate.
    """
    try:
        from jsonschema import Draft202012Validator, FormatChecker
    except ImportError as exc:
        raise RuntimeError("RAG-003 schema validation requires jsonschema==4.23.0") from exc
    with SCHEMA_PATH.open(encoding="utf-8") as handle:
        schema = json.load(handle, object_pairs_hook=unique_object)
    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema, format_checker=FormatChecker())
    errors = []
    for issue in validator.iter_errors(data):
        location = ".".join(str(part) for part in issue.absolute_path) or "<root>"
        errors.append(f"schema {location}: {issue.message[:300]}")
        if len(errors) >= 20:
            errors.append("additional schema errors suppressed")
            break
    return errors

def validate_manifest(data):
    """Reject structurally invalid objects before running semantic checks."""
    problems = validate_structure(data)
    return problems if problems else validate(data)

def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest")
    args = parser.parse_args()
    try:
        with open(args.manifest, "rb") as handle:
            raw = handle.read(MAX_MANIFEST_BYTES + 1)
        if len(raw) > MAX_MANIFEST_BYTES:
            raise ValueError("manifest exceeds 2 MiB")
        data = json.loads(raw.decode("utf-8"), object_pairs_hook=unique_object)
        errors = validate_manifest(data)
    except (OSError, ValueError, UnicodeError, RuntimeError) as exc:
        errors = [str(exc)]
    for error in errors:
        print(error, file=sys.stderr)
    if not errors:
        print("Semantic checks passed; provenance and reviewer authenticity NOT established")
    return bool(errors)

if __name__ == "__main__":
    sys.exit(main())
