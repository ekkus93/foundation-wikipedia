#!/usr/bin/env python3
"""Deterministic pre-review source-revision sampling; does not authenticate Wikimedia."""
import argparse
import hashlib
import json
import re
import sys
from pathlib import Path

MAX_PLAN_BYTES = 1024 * 1024
MAX_INVENTORY_BYTES = 32 * 1024 * 1024
MAX_CANDIDATES = 100_000
PROJECT = re.compile(r"[a-z][a-z0-9_-]{1,31}\Z")


def unique_object(pairs):
    result = {}
    for name, value in pairs:
        if name in result:
            raise ValueError("duplicate JSON key: " + name)
        result[name] = value
    return result


def read_pinned_input(path, budget):
    """Use the exact, size-limited bytes that the output receipt later identifies."""
    with open(path, "rb") as stream:
        raw = stream.read(budget + 1)
    if not raw or len(raw) > budget:
        raise ValueError("input empty or exceeds size budget")
    return json.loads(raw.decode("utf-8"), object_pairs_hook=unique_object), hashlib.sha256(raw).hexdigest()


def _positive(value):
    """Positive Wikimedia source IDs are not inventory-size budgets."""
    return type(value) is int and 0 < value <= (1 << 63) - 1


def _bounded_floor(value):
    return type(value) is int and 0 < value <= MAX_CANDIDATES


def _project(value):
    return isinstance(value, str) and PROJECT.fullmatch(value) is not None


def _distinct_strings(value, valid):
    return (isinstance(value, list) and bool(value)
            and all(valid(item) for item in value)
            and len(value) == len(set(value)))


def _page_key(value):
    if not isinstance(value, dict) or set(value) != {"project", "page_id"}:
        raise ValueError("excluded page keys must contain project and page_id")
    if not _project(value["project"]) or not _positive(value["page_id"]):
        raise ValueError("invalid excluded page key")
    return value["project"], value["page_id"]


def build_roster(plan, inventory):
    """Choose the smallest greedy per-project coverage set with SHA-256-seeded ties.

    Output is a selection receipt, not publisher provenance, true stratum labels,
    preregistration timing, or independently reviewed evaluation evidence.
    """
    expected = {"schema_version", "sampling_seed", "required_projects", "strata",
                "minimum_articles_per_project"}
    if not isinstance(plan, dict) or set(plan) - (expected | {"excluded_page_keys"}):
        raise ValueError("unknown sampling plan fields")
    if not expected.issubset(plan) or type(plan["schema_version"]) is not int or plan["schema_version"] != 1:
        raise ValueError("missing fields or unsupported sampling plan version")
    seed, floor = plan["sampling_seed"], plan["minimum_articles_per_project"]
    if type(seed) is not int or not 0 <= seed < (1 << 64) or not _bounded_floor(floor):
        raise ValueError("invalid sampling seed or minimum article floor")
    projects, strata = plan["required_projects"], plan["strata"]
    if not _distinct_strings(projects, _project):
        raise ValueError("invalid or duplicated required projects")
    if not _distinct_strings(strata, lambda x: isinstance(x, str) and
                             0 < len(x) <= 64 and x.strip() == x and
                             all(ord(c) >= 32 for c in x)):
        raise ValueError("invalid or duplicated required strata")
    if not isinstance(inventory, list) or not 0 < len(inventory) <= MAX_CANDIDATES:
        raise ValueError("invalid candidate inventory size")
    exclusions = plan.get("excluded_page_keys", [])
    if not isinstance(exclusions, list) or len(exclusions) > MAX_CANDIDATES:
        raise ValueError("invalid excluded page-key list")
    excluded = [_page_key(item) for item in exclusions]
    if len(excluded) != len(set(excluded)):
        raise ValueError("duplicate excluded page key")
    if any(project not in projects for project, _ in excluded):
        raise ValueError("excluded key belongs to unplanned project")
    excluded = set(excluded)

    seen_pages, pools = set(), {project: [] for project in projects}
    allowed = {"project", "page_id", "revision_id", "strata"}
    for position, candidate in enumerate(inventory):
        if not isinstance(candidate, dict) or set(candidate) != allowed:
            raise ValueError(f"candidate[{position}]: invalid record fields")
        project, page_id, revision = (candidate[k] for k in
                                      ("project", "page_id", "revision_id"))
        if project not in pools or not _positive(page_id) or not _positive(revision):
            raise ValueError(f"candidate[{position}]: invalid or out-of-plan identity")
        key = (project, page_id)
        if key in seen_pages:
            raise ValueError(f"candidate[{position}]: duplicate page or competing revisions")
        seen_pages.add(key)
        labels = candidate["strata"]
        if not _distinct_strings(labels, lambda x: isinstance(x, str) and x in strata):
            raise ValueError(f"candidate[{position}]: invalid strata labels")
        if key not in excluded:
            pools[project].append({"project": project, "page_id": page_id,
                                   "revision_id": revision, "strata": sorted(labels)})

    selections = []
    counts = {}
    for project in sorted(projects):
        pool = pools[project]
        if len(pool) < floor:
            raise ValueError(f"{project}: too few eligible distinct articles")
        for stratum in strata:
            if not any(stratum in candidate["strata"] for candidate in pool):
                raise ValueError(f"{project}: missing eligible stratum {stratum}")
        missing = set(strata)
        selected = []
        # Compute stable SHA-256 tie ranks once. After stratum coverage is
        # satisfied, sorting the remainder avoids quadratic floor filling.
        def tie_rank(candidate):
            identity = f'{seed}:{candidate["project"]}:{candidate["page_id"]}:{candidate["revision_id"]}'
            return (hashlib.sha256(identity.encode("ascii")).hexdigest(),
                    candidate["page_id"], candidate["revision_id"])

        ranked = sorted(((tie_rank(item), item) for item in pool),
                        key=lambda pair: pair[0])
        while missing:
            index = min(range(len(ranked)),
                        key=lambda i: (-len(missing.intersection(ranked[i][1]["strata"])),
                                       ranked[i][0]))
            _, choice = ranked.pop(index)
            selected.append(choice)
            missing.difference_update(choice["strata"])
        # Remaining candidates are already in deterministic tie order.
        selected.extend(item for _, item in ranked[:max(0, floor - len(selected))])
        counts[project] = {"selected_articles": len(selected),
                           "covered_strata": sorted(set(strata) - missing)}
        selections.extend(selected)
    return {"schema_version": 1, "selection_policy": "greedy-strata-sha256-v1",
            "sampling_seed": seed, "required_projects": sorted(projects),
            "required_strata": sorted(strata), "minimum_articles_per_project": floor,
            "excluded_page_keys": len(excluded), "candidate_count": len(inventory),
            "counts": counts, "selected": selections}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--plan", required=True, type=Path)
    parser.add_argument("--inventory", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path,
                        help="New output path; existing files are never overwritten")
    args = parser.parse_args(argv)
    try:
        plan, plan_hash = read_pinned_input(args.plan, MAX_PLAN_BYTES)
        candidates, inventory_hash = read_pinned_input(args.inventory, MAX_INVENTORY_BYTES)
        roster = build_roster(plan, candidates)
        roster["plan_sha256"] = plan_hash
        roster["inventory_sha256"] = inventory_hash
        payload = (json.dumps(roster, sort_keys=True, ensure_ascii=False,
                              separators=(",", ":")) + "\n").encode("utf-8")
        with args.output.open("xb") as output:
            try:
                output.write(payload)
                output.flush()
            except BaseException:
                output.close()
                args.output.unlink()
                raise
        print(f"Selected {len(roster['selected'])} source revisions; "
              "labels and publisher provenance require independent verification")
        return 0
    except (OSError, ValueError, UnicodeError, TypeError) as exc:
        print(f"sampling failed: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
