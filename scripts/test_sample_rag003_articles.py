"""Synthetic tests: no Wikimedia data, reviewer identities, or scores are fabricated."""
import hashlib
import json
import pathlib
import tempfile
import unittest

from sample_rag003_articles import build_roster, main, read_pinned_input


def plan():
    return {"schema_version": 1, "sampling_seed": 42,
            "required_projects": ["frwiki", "enwiki"],
            "strata": ["factual", "table"],
            "minimum_articles_per_project": 2,
            "excluded_page_keys": [{"project": "enwiki", "page_id": 99}]}


def candidates():
    return [
        {"project": project, "page_id": number, "revision_id": number + 100,
         "strata": tags}
        for project in ("enwiki", "frwiki")
        for number, tags in ((1, ["factual"]), (2, ["table"]),
                             (3, ["factual", "table"]), (4, ["factual"]),
                             (99, ["factual", "table"]))
    ]


class TestSampling(unittest.TestCase):
    def test_deterministic_and_stratum_complete(self):
        p, items = plan(), candidates()
        expected = build_roster(p, items)
        self.assertEqual(expected, build_roster(
            {**p, "required_projects": list(reversed(p["required_projects"])),
             "strata": list(reversed(p["strata"]))},
            [{**item, "strata": list(reversed(item["strata"]))}
             for item in reversed(items)]))
        self.assertEqual(set(expected["counts"]), {"enwiki", "frwiki"})
        for project in ("enwiki", "frwiki"):
            chosen = [item for item in expected["selected"] if item["project"] == project]
            self.assertGreaterEqual(len(chosen), 2)
            self.assertEqual(set.union(*(set(item["strata"]) for item in chosen)),
                             {"factual", "table"})
        self.assertFalse(any(item["project"] == "enwiki" and item["page_id"] == 99
                             for item in expected["selected"]))

    def test_duplicates_and_competing_revisions_rejected(self):
        with self.assertRaisesRegex(ValueError, "duplicate page"):
            build_roster(plan(), candidates() + [candidates()[0]])
        duplicate = {**candidates()[0], "revision_id": 999}
        with self.assertRaisesRegex(ValueError, "competing revisions"):
            build_roster(plan(), candidates() + [duplicate])

    def test_unknown_stratum_and_unplanned_project_fail_closed(self):
        items = candidates()
        items[0] = {**items[0], "strata": ["unregistered"]}
        with self.assertRaisesRegex(ValueError, "invalid strata"):
            build_roster(plan(), items)
        items = candidates()
        items[0] = {**items[0], "project": "dewiki"}
        with self.assertRaisesRegex(ValueError, "out-of-plan"):
            build_roster(plan(), items)

    def test_unsatisfied_coverage_fails_instead_of_cherry_picking(self):
        items = [item for item in candidates() if
                 item["project"] != "frwiki" or "table" not in item["strata"]]
        with self.assertRaisesRegex(ValueError, "frwiki: missing eligible stratum table"):
            build_roster(plan(), items)
        p = plan()
        p["minimum_articles_per_project"] = 10
        with self.assertRaisesRegex(ValueError, "too few eligible"):
            build_roster(p, candidates())

    def test_large_wikimedia_source_ids(self):
        items = candidates()
        items[0] = {**items[0], "page_id": 12_345_678,
                    "revision_id": 1_234_567_890}
        roster = build_roster(plan(), items)
        self.assertEqual(roster["counts"]["enwiki"]["covered_strata"],
                         ["factual", "table"])
        p = plan()
        p["excluded_page_keys"].append({"project": "enwiki", "page_id": 12_345_678})
        self.assertFalse(any(x["page_id"] == 12_345_678
                             for x in build_roster(p, items)["selected"]))

    def test_out_of_range_source_ids_and_floor(self):
        items = candidates()
        items[0] = {**items[0], "revision_id": 1 << 64}
        with self.assertRaisesRegex(ValueError, "invalid or out-of-plan"):
            build_roster(plan(), items)
        p = plan()
        p["minimum_articles_per_project"] = 100_001
        with self.assertRaisesRegex(ValueError, "invalid sampling seed or minimum"):
            build_roster(p, candidates())

    def test_invalid_exclusions_and_seed(self):
        p = plan()
        p["excluded_page_keys"] *= 2
        with self.assertRaisesRegex(ValueError, "duplicate excluded"):
            build_roster(p, candidates())
        p = plan()
        p["sampling_seed"] = True
        with self.assertRaisesRegex(ValueError, "invalid sampling seed"):
            build_roster(p, candidates())
        p = plan()
        p["excluded_page_keys"] = [{"project": "dewiki", "page_id": 9}]
        with self.assertRaisesRegex(ValueError, "unplanned project"):
            build_roster(p, candidates())

    def test_input_bytes_are_bounded_and_duplicate_keys_rejected(self):
        with tempfile.TemporaryDirectory() as folder:
            path = pathlib.Path(folder, "input.json")
            path.write_bytes(b'{"x":1,"x":2}')
            with self.assertRaisesRegex(ValueError, "duplicate JSON key"):
                read_pinned_input(path, 1024)
            path.write_bytes(b" " * 20)
            with self.assertRaisesRegex(ValueError, "exceeds size budget"):
                read_pinned_input(path, 8)

    def test_cli_receipts_and_output_no_clobber(self):
        with tempfile.TemporaryDirectory() as folder:
            root = pathlib.Path(folder)
            plan_path, source_path, output = (root / name for name in
                                              ("plan.json", "inventory.json", "roster.json"))
            plan_path.write_text(json.dumps(plan()), encoding="utf-8")
            source_path.write_text(json.dumps(candidates()), encoding="utf-8")
            args = ["--plan", str(plan_path), "--inventory", str(source_path),
                    "--output", str(output)]
            self.assertEqual(main(args), 0)
            receipt = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(receipt["plan_sha256"],
                             hashlib.sha256(plan_path.read_bytes()).hexdigest())
            self.assertEqual(receipt["inventory_sha256"],
                             hashlib.sha256(source_path.read_bytes()).hexdigest())
            before = output.read_bytes()
            self.assertEqual(main(args), 1)
            self.assertEqual(output.read_bytes(), before)


if __name__ == "__main__":
    unittest.main()
