#!/usr/bin/env python3
"""RAG-003 semantic manifest checks; not a substitute for JSON Schema or authentication."""
import argparse
import datetime
import json
import re
import sys

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
        identities, articles, cases, judgments = set(), {}, {}, set()
        for i, source in enumerate(sources):
            identity = (source["project"], source["page_id"], source["revision_id"])
            if identity in identities:
                errors.append(f"source_articles[{i}]: duplicate revision")
            identities.add(identity)
            articles.setdefault(identity[0], set()).add(identity[1])
            if timestamp(source["acquired_at"]) < registered:
                errors.append(f"source_articles[{i}]: acquisition predates registration")
        for i, review in enumerate(reviews):
            identity = (review["source_project"], review["source_page_id"], review["source_revision_id"])
            if identity not in identities:
                errors.append(f"reviews[{i}]: unknown source revision")
            else:
                cases.setdefault(identity[0], set()).add(review["case_id"])
            key = (review["case_id"], review["reviewer_id"], review["claim_start_byte"], review["claim_end_byte"])
            if key in judgments:
                errors.append(f"reviews[{i}]: duplicate reviewer judgment")
            judgments.add(key)
            if review["claim_start_byte"] < 0 or review["claim_end_byte"] <= review["claim_start_byte"]:
                errors.append(f"reviews[{i}]: invalid claim byte span")
            if timestamp(review["reviewed_at"]) < registered:
                errors.append(f"reviews[{i}]: review predates registration")
        for project in required:
            if len(articles.get(project, set())) < plan["minimum_articles_per_project"]:
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
        with open(args.manifest, encoding="utf-8") as handle:
            errors = validate(json.load(handle, object_pairs_hook=unique_object))
    except (OSError, ValueError) as exc:
        errors = [str(exc)]
    for error in errors:
        print(error, file=sys.stderr)
    if not errors:
        print("Semantic checks passed; provenance and reviewer authenticity NOT established")
    return bool(errors)

if __name__ == "__main__":
    sys.exit(main())
