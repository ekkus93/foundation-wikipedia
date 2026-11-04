"""Synthetic tests for RAG-003 review-to-sampling identity binding."""
import hashlib
import json
import pathlib
import tempfile
import unittest

from sample_rag003_articles import main as sample_main
from test_validate_rag003_review_manifest import complete_schema_fixture
from verify_rag003_review_selection import main, verify_review_selection


class TestReviewedSelection(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        folder = pathlib.Path(self.temp.name)
        self.plan_path = folder / "plan.json"
        self.inventory_path = folder / "inventory.json"
        self.receipt_path = folder / "receipt.json"
        self.manifest_path = folder / "manifest.json"
        self.plan = {"schema_version": 1, "sampling_seed": 42,
                     "required_projects": ["enwiki"], "strata": ["factual"],
                     "minimum_articles_per_project": 1}
        self.inventory = [{"project": "enwiki", "page_id": 1,
                           "revision_id": 2, "strata": ["factual"]}]
        self.manifest = complete_schema_fixture()
        self.regenerate()
        self.write_manifest()

    def regenerate(self):
        self.plan_path.write_text(json.dumps(self.plan), encoding="utf-8")
        self.inventory_path.write_text(json.dumps(self.inventory), encoding="utf-8")
        if self.receipt_path.exists():
            self.receipt_path.unlink()
        self.assertEqual(sample_main(["--plan", str(self.plan_path),
                                      "--inventory", str(self.inventory_path),
                                      "--output", str(self.receipt_path)]), 0)

    def write_manifest(self):
        self.manifest["preregistration"]["plan_sha256"] = hashlib.sha256(
            self.plan_path.read_bytes()).hexdigest()
        self.manifest_path.write_text(json.dumps(self.manifest), encoding="utf-8")

    def check(self):
        return verify_review_selection(self.manifest_path, self.plan_path,
                                       self.inventory_path, self.receipt_path)

    def test_accepted_matching_review(self):
        self.assertEqual(self.check(), [])

    def test_unselected_source_is_rejected(self):
        self.manifest["source_articles"].append({
            **self.manifest["source_articles"][0], "page_id": 5, "revision_id": 6})
        self.write_manifest()
        self.assertTrue(any("unselected revision(s)" in e for e in self.check()))

    def test_substituted_revision_is_rejected_even_with_consistent_reviews(self):
        self.manifest["source_articles"][0]["revision_id"] = 9
        for review in self.manifest["reviews"]:
            review["source_revision_id"] = 9
        self.write_manifest()
        errors = self.check()
        self.assertTrue(any("selected revision(s) missing" in e for e in errors))
        self.assertTrue(any("unselected revision(s) present" in e for e in errors))

    def test_missing_source_from_roster_is_rejected(self):
        self.plan["minimum_articles_per_project"] = 2
        self.inventory.append({"project": "enwiki", "page_id": 7,
                               "revision_id": 8, "strata": ["factual"]})
        self.regenerate()
        self.manifest["preregistration"]["minimum_articles_per_project"] = 2
        self.write_manifest()
        self.assertTrue(any("selected revision(s) missing" in e for e in self.check()))

    def test_sampling_metadata_drift_rejected(self):
        for key, changed in (("sampling_seed", 15),
                             ("strata", ["selection"]),
                             ("required_projects", ["frwiki"]),
                             ("minimum_articles_per_project", 2)):
            with self.subTest(field=key):
                original = self.manifest["preregistration"][key]
                self.manifest["preregistration"][key] = changed
                self.write_manifest()
                self.assertTrue(any(key in e for e in self.check()))
                self.manifest["preregistration"][key] = original
        self.manifest["preregistration"]["plan_sha256"] = "0" * 64
        self.manifest_path.write_text(json.dumps(self.manifest), encoding="utf-8")
        self.assertTrue(any("plan_sha256" in e for e in self.check()))

    def test_tampered_receipt_or_inventory_rejected(self):
        value = json.loads(self.receipt_path.read_text())
        value["selected"][0]["revision_id"] = 10
        self.receipt_path.write_text(json.dumps(value), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "sampling receipt does not match"):
            self.check()
        self.regenerate()
        self.inventory_path.write_text(json.dumps(self.inventory, indent=2), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "sampling receipt does not match"):
            self.check()

    def test_duplicate_manifest_keys_rejected(self):
        self.manifest_path.write_text('{"schema_version":1,"schema_version":1}', encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "duplicate JSON key"):
            self.check()

    def test_cli_status_matches_gate(self):
        args = ["--manifest", str(self.manifest_path), "--plan", str(self.plan_path),
                "--inventory", str(self.inventory_path), "--receipt", str(self.receipt_path)]
        self.assertEqual(main(args), 0)
        self.manifest["source_articles"][0]["revision_id"] = 9
        for review in self.manifest["reviews"]:
            review["source_revision_id"] = 9
        self.write_manifest()
        self.assertEqual(main(args), 1)


if __name__ == "__main__":
    unittest.main()
