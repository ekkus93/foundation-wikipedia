"""Synthetic checks for reproducible preregistered RAG-003 reviewer assignments."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import build_rag003_review_worklist as worklist_module

from sample_rag003_articles import build_roster
from build_rag003_review_worklist import build_worklist, main


class TestReviewWorklist(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        root = Path(self.temp.name)
        self.paths = {key: root / (key + ".json")
                      for key in ("plan", "inventory", "receipt", "reviewers", "output")}
        self.plan = {"schema_version": 1, "sampling_seed": 15,
                     "required_projects": ["enwiki", "frwiki"],
                     "strata": ["factual"], "minimum_articles_per_project": 4}
        self.inventory = [
            {"project": project, "page_id": n, "revision_id": n + 100000000000,
             "strata": ["factual"]}
            for project in ("enwiki", "frwiki") for n in range(1, 7)
        ]
        self.reviewers = {"reviewer_ids": ["reviewer_c", "reviewer_a", "reviewer_b"]}
        self.save()

    def save(self):
        for name, value in (("plan", self.plan), ("inventory", self.inventory),
                            ("reviewers", self.reviewers)):
            self.paths[name].write_text(json.dumps(value), encoding="utf-8")
        roster = build_roster(self.plan, self.inventory)
        for name in ("plan", "inventory"):
            roster[name + "_sha256"] = hashlib.sha256(self.paths[name].read_bytes()).hexdigest()
        self.paths["receipt"].write_text(json.dumps(roster), encoding="utf-8")

    def args(self):
        return [arg for field in ("plan", "inventory", "receipt", "reviewers", "output")
                for arg in ("--" + field, str(self.paths[field]))]

    def run_valid(self):
        self.assertEqual(main(self.args()), 0)
        return json.loads(self.paths["output"].read_text(encoding="utf-8"))

    def test_valid_worklist_is_explicitly_pending_and_revision_scoped(self):
        worklist = self.run_valid()
        self.assertEqual(worklist["source_count"], 8)
        self.assertEqual(worklist["review_slots"], 16)
        self.assertIn("no authenticated source acquisition", worklist["warning"])
        self.assertEqual(len(worklist["assignments"]), 8)
        for assignment in worklist["assignments"]:
            self.assertEqual(assignment["source_acquisition"], "pending")
            self.assertEqual(assignment["independent_reviews"], "pending")
            self.assertEqual(len(set(assignment["assigned_reviewer_ids"])), 2)
            self.assertGreater(assignment["revision_id"], 100000000000)

    def test_balanced_assignments_per_project(self):
        worklist = self.run_valid()
        for project in ("enwiki", "frwiki"):
            counts = {reviewer: 0 for reviewer in self.reviewers["reviewer_ids"]}
            for item in worklist["assignments"]:
                if item["project"] == project:
                    for reviewer in item["assigned_reviewer_ids"]:
                        counts[reviewer] += 1
            self.assertLessEqual(max(counts.values()) - min(counts.values()), 1)

    def test_reproducible_from_identical_pinned_bytes(self):
        first = self.run_valid()
        self.paths["output"].unlink()
        self.assertEqual(self.run_valid(), first)

    def test_forged_receipt_is_rejected(self):
        receipt = json.loads(self.paths["receipt"].read_text())
        receipt["selected"][0]["revision_id"] += 1
        self.paths["receipt"].write_text(json.dumps(receipt), encoding="utf-8")
        self.assertEqual(main(self.args()), 1)
        self.assertFalse(self.paths["output"].exists())

    def test_mutated_inventory_bytes_are_rejected(self):
        self.paths["inventory"].write_text(json.dumps(self.inventory, indent=2), encoding="utf-8")
        self.assertEqual(main(self.args()), 1)
        self.assertFalse(self.paths["output"].exists())

    def test_missing_or_duplicate_reviewers_fail_closed(self):
        for reviewers in ({"reviewer_ids": ["only_one"]},
                          {"reviewer_ids": ["one", "one"]},
                          {"reviewer_ids": ["one", "two\nbad"]},
                          {"reviewer_ids": ["one", True]},
                          {"reviewer_ids": ["one", "two"], "completed": True}):
            with self.subTest(reviewers=reviewers):
                self.reviewers = reviewers
                self.save()
                self.assertEqual(main(self.args()), 1)
                self.assertFalse(self.paths["output"].exists())

    def test_never_overwrites_existing_output(self):
        self.paths["output"].write_text("existing", encoding="utf-8")
        self.assertEqual(main(self.args()), 1)
        self.assertEqual(self.paths["output"].read_text(), "existing")

    def test_duplicate_json_keys_in_reviewer_roster_are_rejected(self):
        self.paths["reviewers"].write_text(
            '{"reviewer_ids":["one","two"],"reviewer_ids":["one","two"]}',
            encoding="utf-8")
        self.assertEqual(main(self.args()), 1)
        self.assertFalse(self.paths["output"].exists())

    def test_missing_required_input_leaves_no_output(self):
        self.paths["receipt"].unlink()
        self.assertEqual(main(self.args()), 1)
        self.assertFalse(self.paths["output"].exists())

    def test_direct_builder_rejects_oversized_selection(self):
        # Keep resource test cheap by using a valid small fixture and directly
        # checking the full bounded plan/receipt path in the CLI elsewhere.
        self.reviewers = {"reviewer_ids": ["one", "two"]}
        self.save()
        values = {name: json.loads(self.paths[name].read_text())
                  for name in ("plan", "inventory", "receipt", "reviewers")}
        digest = {name: hashlib.sha256(self.paths[name].read_bytes()).hexdigest()
                  for name in values}
        result = build_worklist(*(values[name] for name in values),
                                *(digest[name] for name in values))
        self.assertEqual(result["review_slots"], 16)
        # Exercise the capacity guard with a lowered ceiling rather than
        # allocating a 10,001-article synthetic fixture.
        with patch.object(worklist_module, "MAX_ASSIGNED_ARTICLES", 7):
            with self.assertRaisesRegex(ValueError, "exceeds 10,000"):
                build_worklist(*(values[name] for name in values),
                               *(digest[name] for name in values))
            self.assertEqual(main(self.args()), 1)
            self.assertFalse(self.paths["output"].exists())

    def test_reviewer_roster_changes_require_new_pinned_worklist(self):
        original = self.run_valid()
        self.paths["output"].unlink()
        self.reviewers = {"reviewer_ids": ["reviewer_a", "reviewer_b"]}
        self.save()
        changed = self.run_valid()
        self.assertNotEqual(original["reviewer_roster_sha256"],
                            changed["reviewer_roster_sha256"])
        for assignment in changed["assignments"]:
            self.assertEqual(set(assignment["assigned_reviewer_ids"]),
                             {"reviewer_a", "reviewer_b"})
            self.assertEqual(assignment["independent_reviews"], "pending")


if __name__ == "__main__":
    unittest.main()
