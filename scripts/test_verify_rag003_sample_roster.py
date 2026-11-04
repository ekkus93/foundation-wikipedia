"""Synthetic verification regression for pinned RAG-003 sampling receipts."""
import copy
import json
import pathlib
import tempfile
import unittest

from sample_rag003_articles import main as sample_main
from test_sample_rag003_articles import candidates, plan
from verify_rag003_sample_roster import main as verify_main


class TestReplay(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = pathlib.Path(self.tmp.name)
        self.plan_path = root / "plan.json"
        self.inventory_path = root / "inventory.json"
        self.receipt_path = root / "roster.json"
        self.plan_path.write_text(json.dumps(plan()), encoding="utf-8")
        self.inventory_path.write_text(json.dumps(candidates()), encoding="utf-8")
        self.assertEqual(sample_main(["--plan", str(self.plan_path),
                                      "--inventory", str(self.inventory_path),
                                      "--output", str(self.receipt_path)]), 0)

    def replay(self):
        return verify_main(["--plan", str(self.plan_path),
                            "--inventory", str(self.inventory_path),
                            "--receipt", str(self.receipt_path)])

    def test_good_receipt_replays(self):
        self.assertEqual(self.replay(), 0)

    def test_modified_selected_revision_rejected(self):
        data = json.loads(self.receipt_path.read_text())
        data["selected"][0]["revision_id"] += 1
        self.receipt_path.write_text(json.dumps(data))
        self.assertEqual(self.replay(), 1)

    def test_missing_or_reordered_selected_items_rejected(self):
        original = json.loads(self.receipt_path.read_text())
        for change in ("missing", "reordered"):
            data = copy.deepcopy(original)
            if change == "missing":
                data["selected"].pop()
            else:
                data["selected"].reverse()
            self.receipt_path.write_text(json.dumps(data))
            self.assertEqual(self.replay(), 1)

    def test_input_mutation_rejected(self):
        self.inventory_path.write_text(json.dumps(list(reversed(candidates()))))
        self.assertEqual(self.replay(), 1)

    def test_extra_receipt_fields_rejected(self):
        data = json.loads(self.receipt_path.read_text())
        data["independent_review_passed"] = True
        self.receipt_path.write_text(json.dumps(data))
        self.assertEqual(self.replay(), 1)

    def test_boolean_count_cannot_masquerade_as_integer(self):
        data = json.loads(self.receipt_path.read_text())
        data["excluded_page_keys"] = True
        self.receipt_path.write_text(json.dumps(data))
        self.assertEqual(self.replay(), 1)

    def test_duplicate_json_keys_rejected(self):
        raw = self.receipt_path.read_text()
        self.receipt_path.write_text(raw.replace('"schema_version":1',
                                                  '"schema_version":1,"schema_version":1'))
        self.assertEqual(self.replay(), 1)


if __name__ == "__main__":
    unittest.main()
