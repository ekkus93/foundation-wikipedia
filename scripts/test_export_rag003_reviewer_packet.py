"""Private reviewer packet export, pinned replay and fail-closed regression."""
import json
import os
import stat
from unittest.mock import patch
import unittest
from pathlib import Path

from test_build_rag003_review_worklist import TestReviewWorklist
from export_rag003_reviewer_packet import main


class TestReviewerPacket(TestReviewWorklist):
    def packet_args(self, reviewer="reviewer_a"):
        return [value for name in ("plan", "inventory", "receipt", "reviewers", "worklist")
                for value in ("--" + name, str(self.paths["output" if name == "worklist" else name]))] + [
                    "--reviewer-id", reviewer, "--output", str(self.paths["output"].with_suffix(".packet"))]

    def test_packet_excludes_peer_reviewer_identities(self):
        self.run_valid()
        self.assertEqual(main(self.packet_args()), 0)
        packet = json.loads(self.paths["output"].with_suffix(".packet").read_text())
        self.assertEqual(packet["reviewer_id"], "reviewer_a")
        self.assertGreater(packet["assignment_count"], 0)
        self.assertEqual(packet["assignment_count"], len(packet["assignments"]))
        self.assertNotIn("reviewer_ids", packet)
        for row in packet["assignments"]:
            self.assertNotIn("assigned_reviewer_ids", row)
            self.assertEqual(row["independent_review"], "pending")
            self.assertEqual(row["source_acquisition"], "pending")

    def test_unassigned_reviewer_rejected_without_packet(self):
        self.run_valid()
        self.assertEqual(main(self.packet_args("outsider")), 1)
        self.assertFalse(self.paths["output"].with_suffix(".packet").exists())

    def test_tampered_worklist_rejected_before_export(self):
        value = self.run_valid()
        value["assignments"][0]["independent_reviews"] = "complete"
        self.paths["output"].write_text(json.dumps(value))
        self.assertEqual(main(self.packet_args()), 1)
        self.assertFalse(self.paths["output"].with_suffix(".packet").exists())

    @unittest.skipUnless(os.name == "posix", "POSIX file permissions")
    def test_packet_is_private_under_permissive_umask(self):
        self.run_valid()
        previous = os.umask(0)
        try:
            self.assertEqual(main(self.packet_args()), 0)
        finally:
            os.umask(previous)
        packet = self.paths["output"].with_suffix(".packet")
        self.assertEqual(stat.S_IMODE(packet.stat().st_mode), 0o600)
        self.assertEqual(json.loads(packet.read_text())["reviewer_id"], "reviewer_a")

    def test_failed_publication_cleans_private_temporary_file(self):
        self.run_valid()
        with patch("rag003_private_output.os.link", side_effect=OSError("publish failure")):
            self.assertEqual(main(self.packet_args()), 1)
        self.assertFalse(self.paths["output"].with_suffix(".packet").exists())
        self.assertEqual(list(self.paths["output"].parent.glob(".rag003-private-*")), [])

    @unittest.skipUnless(hasattr(os, "symlink"), "symbolic links unavailable")
    def test_existing_symlink_is_not_followed_or_replaced(self):
        self.run_valid()
        target = self.paths["output"].parent / "existing.txt"
        target.write_text("untouched")
        self.paths["output"].with_suffix(".packet").symlink_to(target)
        self.assertEqual(main(self.packet_args()), 1)
        self.assertEqual(target.read_text(), "untouched")
        self.assertTrue(self.paths["output"].with_suffix(".packet").is_symlink())

    def test_packet_does_not_overwrite_existing_file(self):
        self.run_valid()
        path = self.paths["output"].with_suffix(".packet")
        path.write_text("existing")
        self.assertEqual(main(self.packet_args()), 1)
        self.assertEqual(path.read_text(), "existing")

    def test_reviewer_roster_byte_change_rejected(self):
        self.run_valid()
        self.paths["reviewers"].write_text(json.dumps(self.reviewers, indent=2))
        self.assertEqual(main(self.packet_args()), 1)
        self.assertFalse(self.paths["output"].with_suffix(".packet").exists())


if __name__ == "__main__":
    unittest.main()
