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
        "reviews": [{"case_id": "c1", "reviewer_id": "r1", "rubric_version": "v1", "claim_start_byte": 0, "claim_end_byte": 2, "source_project": "enwiki", "source_page_id": 1, "source_revision_id": 2, "reviewed_at": "2026-11-02T01:00:00Z"}],
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
    def test_duplicate_source_cannot_qualify_coverage(self):
        data = fixture()
        data["source_articles"].append(copy.deepcopy(data["source_articles"][0]))
        errors = validator.validate(data)
        self.assertTrue(any("duplicate revision" in e for e in errors))
        self.assertTrue(any("insufficient reviewed cases" in e for e in errors))
        self.assertTrue(any("insufficient distinct articles" in e for e in errors))

    def test_duplicate_source_cannot_replace_acquisition_time(self):
        data = fixture()
        duplicate = copy.deepcopy(data["source_articles"][0])
        duplicate["acquired_at"] = "2026-11-02T02:00:00Z"
        data["source_articles"].append(duplicate)
        errors = validator.validate(data)
        self.assertTrue(any("duplicate revision" in e for e in errors))
        self.assertFalse(any("review predates source acquisition" in e for e in errors))

    def test_unreviewed_source_does_not_satisfy_article_floor(self):
        data = fixture()
        data["preregistration"]["minimum_articles_per_project"] = 2
        data["source_articles"].append({**data["source_articles"][0], "page_id": 3, "revision_id": 4})
        self.assertTrue(any("insufficient distinct articles" in e for e in validator.validate(data)))

    def test_reviewed_second_article_satisfies_floor(self):
        data = fixture()
        data["preregistration"]["minimum_articles_per_project"] = 2
        data["source_articles"].append({**data["source_articles"][0], "page_id": 3, "revision_id": 4})
        data["reviews"].append({**data["reviews"][0], "case_id": "c2", "source_page_id": 3, "source_revision_id": 4})
        self.assertEqual(validator.validate(data), [])

    def test_case_id_cannot_span_projects(self):
        data = fixture()
        data["source_articles"].append({**data["source_articles"][0], "project": "frwiki"})
        data["reviews"].append({**data["reviews"][0], "source_project": "frwiki", "reviewer_id": "r2"})
        self.assertTrue(any("case_id reused across projects" in e for e in validator.validate(data)))

    def test_case_id_cannot_span_articles_within_project(self):
        data = fixture()
        data["preregistration"]["minimum_articles_per_project"] = 2
        data["source_articles"].append({**data["source_articles"][0], "page_id": 3, "revision_id": 4})
        data["reviews"].append({**data["reviews"][0], "reviewer_id": "r2", "source_page_id": 3, "source_revision_id": 4})
        errors = validator.validate(data)
        self.assertTrue(any("case_id reused across source revisions" in e for e in errors))
        self.assertTrue(any("insufficient distinct articles" in e for e in errors))

    def test_case_id_cannot_span_revisions_of_same_article(self):
        data = fixture()
        data["source_articles"].append({**data["source_articles"][0], "revision_id": 5})
        data["reviews"].append({**data["reviews"][0], "reviewer_id": "r2", "source_revision_id": 5})
        self.assertTrue(any("case_id reused across source revisions" in e for e in validator.validate(data)))

    def test_different_case_ids_can_reference_different_articles(self):
        data = fixture()
        data["preregistration"]["minimum_articles_per_project"] = 2
        data["source_articles"].append({**data["source_articles"][0], "page_id": 3, "revision_id": 4})
        data["reviews"].append({**data["reviews"][0], "case_id": "c2", "reviewer_id": "r2", "source_page_id": 3, "source_revision_id": 4})
        self.assertEqual(validator.validate(data), [])

    def test_bad_span(self):
        data = fixture()
        data["reviews"][0]["claim_end_byte"] = 0
        self.assertTrue(any("byte span" in e for e in validator.validate(data)))
    def test_early_review(self):
        data = fixture()
        data["reviews"][0]["reviewed_at"] = "2026-10-01T00:00:00Z"
        self.assertTrue(any("predates" in e for e in validator.validate(data)))
    def test_review_before_source_acquisition(self):
        data = fixture()
        data["source_articles"][0]["acquired_at"] = "2026-11-02T02:00:00Z"
        self.assertTrue(any("review predates source acquisition" in e for e in validator.validate(data)))

    def test_review_after_source_acquisition(self):
        data = fixture()
        data["reviews"][0]["reviewed_at"] = "2026-11-02T02:00:00Z"
        self.assertEqual(validator.validate(data), [])

    def test_premature_review_does_not_satisfy_coverage(self):
        data = fixture()
        data["reviews"][0]["reviewed_at"] = "2026-11-01T12:00:00Z"
        errors = validator.validate(data)
        self.assertTrue(any("predates source acquisition" in e for e in errors))
        self.assertTrue(any("insufficient reviewed cases" in e for e in errors))
        self.assertTrue(any("insufficient distinct articles" in e for e in errors))

    def test_invalid_span_does_not_satisfy_coverage(self):
        data = fixture()
        data["reviews"][0]["claim_end_byte"] = 0
        errors = validator.validate(data)
        self.assertTrue(any("invalid claim byte span" in e for e in errors))
        self.assertTrue(any("insufficient reviewed cases" in e for e in errors))

    def test_duplicate_judgment_does_not_inflate_case_coverage(self):
        data = fixture()
        data["preregistration"]["minimum_cases_per_project"] = 2
        data["reviews"].append(copy.deepcopy(data["reviews"][0]))
        errors = validator.validate(data)
        self.assertTrue(any("duplicate reviewer judgment" in e for e in errors))
        self.assertTrue(any("insufficient reviewed cases" in e for e in errors))

    def test_cross_project_case_does_not_count_for_second_project(self):
        data = fixture()
        data["preregistration"]["required_projects"].append("frwiki")
        data["source_articles"].append({**data["source_articles"][0], "project": "frwiki"})
        data["reviews"].append({**data["reviews"][0], "source_project": "frwiki", "reviewer_id": "r2"})
        errors = validator.validate(data)
        self.assertTrue(any("case_id reused across projects" in e for e in errors))
        self.assertTrue(any("frwiki: insufficient reviewed cases" in e for e in errors))

    def test_same_reviewer_conflicting_claim_span_rejected(self):
        data = fixture()
        data["preregistration"]["minimum_cases_per_project"] = 2
        data["reviews"].append({**data["reviews"][0], "claim_start_byte": 2, "claim_end_byte": 4})
        errors = validator.validate(data)
        self.assertTrue(any("conflicting claim span" in e for e in errors))
        self.assertTrue(any("insufficient reviewed cases" in e for e in errors))

    def test_two_reviewers_conflicting_claim_span_rejected(self):
        data = fixture()
        data["reviews"].append({**data["reviews"][0], "reviewer_id": "r2", "claim_start_byte": 2, "claim_end_byte": 4})
        errors = validator.validate(data)
        self.assertTrue(any("conflicting claim span" in e for e in errors))

    def test_two_reviewers_same_case_same_span_allowed(self):
        data = fixture()
        data["reviews"].append({**data["reviews"][0], "reviewer_id": "r2"})
        self.assertEqual(validator.validate(data), [])

    def test_reviewers_with_conflicting_rubrics_rejected(self):
        data = fixture()
        data["reviews"].append({**data["reviews"][0], "reviewer_id": "r2", "rubric_version": "v2"})
        errors = validator.validate(data)
        self.assertTrue(any("conflicting rubric version" in e for e in errors))

    def test_matching_rubrics_across_reviewers_allowed(self):
        data = fixture()
        data["reviews"].append({**data["reviews"][0], "reviewer_id": "r2"})
        self.assertEqual(validator.validate(data), [])

    def test_conflicting_rubric_cannot_qualify_second_case(self):
        data = fixture()
        data["preregistration"]["minimum_cases_per_project"] = 2
        data["reviews"].append({**data["reviews"][0], "reviewer_id": "r2", "rubric_version": "v2"})
        errors = validator.validate(data)
        self.assertTrue(any("conflicting rubric version" in e for e in errors))
        self.assertTrue(any("insufficient reviewed cases" in e for e in errors))

    def test_duplicate_json_key(self):
        with self.assertRaises(ValueError):
            validator.unique_object([("a", 1), ("a", 2)])


def complete_schema_fixture():
    """Structurally valid fixture: synthetic, NOT evidence of real reviewers."""
    data = fixture()
    data["schema_version"] = 1
    data["evaluation_id"] = "synthetic_review_validation"
    data["preregistration"].update({
        "sampling_seed": 42, "strata": ["factual"],
        "plan_sha256": H,
    })
    data["source_articles"][0].update({
        "source_url": "https://en.wikipedia.org/wiki/Example",
        "generation_id": "synthetic_generation",
        "sha256": H,
        "verification_method": "fixture_only",
    })
    data["reviews"][0].update({
        "verdict": "supported", "source_span": "fixture passage",
        "citation_valid": True,
    })
    data["execution"]["command"] = "synthetic test only"
    return data

class TestManifestStructure(unittest.TestCase):
    def test_full_manifest_accepts_structural_and_semantic_validation(self):
        self.assertEqual(validator.validate_structure(complete_schema_fixture()), [])
        self.assertEqual(validator.validate_manifest(complete_schema_fixture()), [])

    def test_missing_field_rejected_even_if_semantic_fields_exist(self):
        data = complete_schema_fixture()
        del data["source_articles"][0]["source_url"]
        self.assertTrue(any("schema" in error and "source_url" in error
                            for error in validator.validate_manifest(data)))

    def test_unknown_field_and_bad_types_rejected(self):
        data = complete_schema_fixture()
        data["execution"]["extra_undeclared"] = 123
        self.assertTrue(any("extra_undeclared" in error
                            for error in validator.validate_manifest(data)))
        data = complete_schema_fixture()
        data["reviews"][0]["citation_valid"] = "yes"
        self.assertTrue(any("citation_valid" in error
                            for error in validator.validate_manifest(data)))

    def test_format_assertions_reject_invalid_timestamp_and_url(self):
        data = complete_schema_fixture()
        data["source_articles"][0]["acquired_at"] = "2026-99-99T00:00:00Z"
        self.assertTrue(any("acquired_at" in error
                            for error in validator.validate_manifest(data)))
        data = complete_schema_fixture()
        data["source_articles"][0]["source_url"] = "http://localhost/source"
        self.assertTrue(any("source_url" in error
                            for error in validator.validate_manifest(data)))

    def test_semantic_errors_still_reported_for_schema_valid_data(self):
        data = complete_schema_fixture()
        data["reviews"][0]["source_revision_id"] = 999
        self.assertEqual(validator.validate_structure(data), [])
        self.assertTrue(any("unknown source revision" in error
                            for error in validator.validate_manifest(data)))

    def test_main_rejects_invalid_structure_and_accepts_valid_fixture(self):
        import contextlib
        import io
        import json
        import sys
        import tempfile
        from unittest.mock import patch

        with tempfile.TemporaryDirectory() as folder:
            path = pathlib.Path(folder) / "manifest.json"
            with patch.object(sys, "argv", ["validator", str(path)]):
                path.write_text(json.dumps(complete_schema_fixture()), encoding="utf-8")
                with contextlib.redirect_stdout(io.StringIO()):
                    self.assertFalse(validator.main())
                malformed = complete_schema_fixture()
                malformed["schema_version"] = 999
                path.write_text(json.dumps(malformed), encoding="utf-8")
                with contextlib.redirect_stderr(io.StringIO()):
                    self.assertTrue(validator.main())
                path.write_text('{"schema_version":1,"schema_version":1}', encoding="utf-8")
                with contextlib.redirect_stderr(io.StringIO()):
                    self.assertTrue(validator.main())

    def test_size_limit_rejects_oversized_file(self):
        import contextlib
        import io
        import sys
        import tempfile
        from unittest.mock import patch

        with tempfile.TemporaryDirectory() as folder:
            path = pathlib.Path(folder) / "oversize.json"
            path.write_bytes(b"x" * (validator.MAX_MANIFEST_BYTES + 1))
            with patch.object(sys, "argv", ["validator", str(path)]):
                with contextlib.redirect_stderr(io.StringIO()):
                    self.assertTrue(validator.main())

if __name__ == "__main__":
    unittest.main()
