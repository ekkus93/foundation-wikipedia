import copy
import importlib.util
import pathlib
import unittest

spec = importlib.util.spec_from_file_location("validator", pathlib.Path(__file__).with_name("validate_rag003_review_manifest.py"))
validator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(validator)
H = "a" * 64

def fixture():
    return {
        "preregistration": {"registered_at": "2026-11-01T00:00:00Z", "required_projects": ["enwiki"], "minimum_cases_per_project": 1, "minimum_articles_per_project": 1},
        "source_articles": [{"project": "enwiki", "page_id": 1, "revision_id": 2, "acquired_at": "2026-11-02T00:00:00Z"}],
        "reviews": [{"case_id": "c1", "reviewer_id": "r1", "claim_start_byte": 0, "claim_end_byte": 2, "source_project": "enwiki", "source_page_id": 1, "source_revision_id": 2, "reviewed_at": "2026-11-02T01:00:00Z"}],
        "execution": {"runner_commit": "b" * 40, "suite_sha256": H, "corpus_sha256": H, "report_sha256": H, "model_config_sha256": H},
    }

class TestManifest(unittest.TestCase):
    def test_valid(self):
        self.assertEqual(validator.validate(fixture()), [])
    def test_missing_project(self):
        data = fixture()
        data["preregistration"]["required_projects"].append("frwiki")
        self.assertTrue(any("frwiki" in e for e in validator.validate(data)))
    def test_unknown_revision(self):
        data = fixture()
        data["reviews"][0]["source_revision_id"] = 3
        self.assertTrue(any("unknown source" in e for e in validator.validate(data)))
    def test_duplicate_review(self):
        data = fixture()
        data["reviews"].append(copy.deepcopy(data["reviews"][0]))
        self.assertTrue(any("duplicate reviewer" in e for e in validator.validate(data)))
    def test_duplicate_source(self):
        data = fixture()
        data["source_articles"].append(copy.deepcopy(data["source_articles"][0]))
        self.assertTrue(any("duplicate revision" in e for e in validator.validate(data)))
    def test_bad_span(self):
        data = fixture()
        data["reviews"][0]["claim_end_byte"] = 0
        self.assertTrue(any("byte span" in e for e in validator.validate(data)))
    def test_early_review(self):
        data = fixture()
        data["reviews"][0]["reviewed_at"] = "2026-10-01T00:00:00Z"
        self.assertTrue(any("predates" in e for e in validator.validate(data)))
    def test_duplicate_json_key(self):
        with self.assertRaises(ValueError):
            validator.unique_object([("a", 1), ("a", 2)])

if __name__ == "__main__":
    unittest.main()
