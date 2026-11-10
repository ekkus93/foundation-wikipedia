"""Replay and tamper resistance for reviewer-scoped RAG-003 packets."""
import json
import unittest
from pathlib import Path

from test_build_rag003_review_worklist import TestReviewWorklist
from export_rag003_reviewer_packet import main as export_main
from verify_rag003_reviewer_packet import main as verify_main


class TestPacketVerification(unittest.TestCase):
    # Borrow fixture helpers explicitly without inheriting TestReviewWorklist's
    # test_* methods (which assume that the worklist output does not yet exist).
    save = TestReviewWorklist.save
    args = TestReviewWorklist.args
    run_valid = TestReviewWorklist.run_valid

    def setUp(self):
        # Reuse fixture construction without inheriting the worklist test cases:
        # those tests require an absent output, while packet setup creates one.
        TestReviewWorklist.setUp(self)
        TestReviewWorklist.run_valid(self)
        self.packet_path = self.paths["output"].with_suffix(".packet")
        self.packet_args = [value for name in ("plan", "inventory", "receipt", "reviewers", "worklist")
                            for value in ("--" + name,
                                          str(self.paths["output"] if name == "worklist" else self.paths[name]))]
        self.assertEqual(export_main(self.packet_args +
                                     ["--reviewer-id", "reviewer_a", "--output", str(self.packet_path)]), 0)

    def verify_args(self):
        return self.packet_args + ["--packet", str(self.packet_path)]

    def test_valid_packet(self):
        self.assertEqual(verify_main(self.verify_args()), 0)

    def test_forged_completion(self):
        packet = json.loads(self.packet_path.read_text())
        packet["assignments"][0]["independent_review"] = "completed"
        self.packet_path.write_text(json.dumps(packet))
        self.assertEqual(verify_main(self.verify_args()), 1)

    def test_peer_assignment_injection(self):
        packet = json.loads(self.packet_path.read_text())
        packet["assignments"][0]["assigned_reviewer_ids"] = ["reviewer_b"]
        self.packet_path.write_text(json.dumps(packet))
        self.assertEqual(verify_main(self.verify_args()), 1)

    def test_substituted_revision(self):
        packet = json.loads(self.packet_path.read_text())
        packet["assignments"][0]["revision_id"] += 1
        self.packet_path.write_text(json.dumps(packet))
        self.assertEqual(verify_main(self.verify_args()), 1)

    def test_changed_worklist_bytes(self):
        self.paths["output"].write_text(self.paths["output"].read_text() + " ")
        self.assertEqual(verify_main(self.verify_args()), 1)

    def test_unknown_reviewer(self):
        packet = json.loads(self.packet_path.read_text())
        packet["reviewer_id"] = "outsider"
        self.packet_path.write_text(json.dumps(packet))
        self.assertEqual(verify_main(self.verify_args()), 1)

    def test_duplicate_json_key(self):
        original = self.packet_path.read_text()
        self.packet_path.write_text(original.replace('"schema_version":1',
                                                      '"schema_version":1,"schema_version":1', 1))
        self.assertEqual(verify_main(self.verify_args()), 1)

    def test_extra_claim(self):
        packet = json.loads(self.packet_path.read_text())
        packet["review_complete"] = True
        self.packet_path.write_text(json.dumps(packet))
        self.assertEqual(verify_main(self.verify_args()), 1)


if __name__ == "__main__":
    unittest.main()
