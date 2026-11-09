"""Tamper and deterministic-replay checks for pending review worklists."""
import json
import unittest

from test_build_rag003_review_worklist import TestReviewWorklist
from verify_rag003_review_worklist import main, verify


class TestWorklistReplay(TestReviewWorklist):
    def verify_args(self):
        return [value for name in ("plan", "inventory", "receipt", "reviewers", "worklist")
                for value in ("--" + name, str(self.paths["output" if name == "worklist" else name]))]

    def test_exact_replay(self):
        self.run_valid()
        self.assertEqual(main(self.verify_args()), 0)
        self.assertEqual(verify(*(self.paths[name] for name in
                                  ("plan", "inventory", "receipt", "reviewers", "output"))), 8)

    def test_changed_assignments_rejected(self):
        original = self.run_valid()
        for mutation in ("reviewer", "source", "status", "extra", "boolean"):
            with self.subTest(mutation=mutation):
                value = json.loads(json.dumps(original))
                if mutation == "reviewer":
                    value["assignments"][0]["assigned_reviewer_ids"] = ["reviewer_a", "reviewer_a"]
                elif mutation == "source":
                    value["assignments"][0]["revision_id"] += 1
                elif mutation == "status":
                    value["assignments"][0]["independent_reviews"] = "complete"
                elif mutation == "extra":
                    value["certified"] = True
                else:
                    value["source_count"] = True
                self.paths["output"].write_text(json.dumps(value))
                self.assertEqual(main(self.verify_args()), 1)

    def test_pinned_reviewer_bytes_required(self):
        self.run_valid()
        self.paths["reviewers"].write_text(json.dumps(self.reviewers, indent=2))
        self.assertEqual(main(self.verify_args()), 1)

    def test_duplicate_json_key_rejected(self):
        self.run_valid()
        self.paths["output"].write_text('{"source_count":8,"source_count":8}')
        self.assertEqual(main(self.verify_args()), 1)

    def test_missing_worklist_rejected(self):
        self.assertEqual(main(self.verify_args()), 1)


if __name__ == "__main__":
    unittest.main()
