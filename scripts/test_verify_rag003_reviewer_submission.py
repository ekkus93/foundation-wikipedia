"""RAG-003 reviewer-submission structural intake and fail-closed regressions."""
import hashlib
import json
import os
from pathlib import Path
import stat
import unittest

from export_rag003_reviewer_packet import main as export_main
from test_build_rag003_review_worklist import TestReviewWorklist
from verify_rag003_reviewer_submission import main as submit_main


class TestReviewerSubmission(unittest.TestCase):
    # Reuse fixture construction without inheriting the unrelated worklist tests.
    save = TestReviewWorklist.save
    args = TestReviewWorklist.args
    run_valid = TestReviewWorklist.run_valid

    def setUp(self):
        TestReviewWorklist.setUp(self)
        self.run_valid()
        root = self.paths["output"].parent
        self.packet = root / "private.packet"
        self.submission = root / "submission.json"
        self.report = root / "structural-report.json"
        self.base_args = [arg for name in ("plan", "inventory", "receipt", "reviewers", "worklist")
                          for arg in ("--" + name,
                                      str(self.paths["output"] if name == "worklist"
                                          else self.paths[name]))]
        self.assertEqual(export_main(self.base_args + [
            "--reviewer-id", "reviewer_a", "--output", str(self.packet)]), 0)
        packet = json.loads(self.packet.read_text(encoding="utf-8"))
        first = packet["assignments"][0]
        self.record = {
            "case_id": "case_1", "project": first["project"],
            "page_id": first["page_id"], "revision_id": first["revision_id"],
            "answer_sha256": "a" * 64, "source_artifact_sha256": "b" * 64,
            "claim_start_byte": 0, "claim_end_byte": 12,
            "reviewed_at": "2026-11-10T06:00:00Z", "rubric_version": "v1",
            "verdict": "supported", "source_span": "A cited source sentence",
            "citation_valid": True,
        }
        self.data = {
            "schema_version": 1, "submission_policy": "reviewer-claims-v1",
            "packet_sha256": hashlib.sha256(self.packet.read_bytes()).hexdigest(),
            "reviewer_id": packet["reviewer_id"],
            "judgments": [self.record.copy()],
        }
        self.save_submission()

    def save_submission(self):
        self.submission.write_text(json.dumps(self.data), encoding="utf-8")

    def verify(self, report=False):
        options = self.base_args + ["--packet", str(self.packet),
                                    "--submission", str(self.submission)]
        if report:
            options.extend(["--report", str(self.report)])
        return submit_main(options)

    def test_valid_submission_is_not_evidence_of_human_or_source_authentication(self):
        self.assertEqual(self.verify(report=True), 0)
        receipt = json.loads(self.report.read_text(encoding="utf-8"))
        self.assertEqual(receipt["judgment_count"], 1)
        self.assertEqual(receipt["sources_with_submitted_judgments"], 1)
        self.assertEqual(receipt["assigned_source_count"], json.loads(self.packet.read_text())["assignment_count"])
        for name in ("reviewer_identity_authenticated", "source_artifact_hashes_verified",
                     "answer_claims_factually_verified", "independent_human_review_certified"):
            self.assertIs(receipt[name], False)
        self.assertEqual(receipt["qualification_status"], "not_qualified")

    def test_valid_multiple_claims_same_case(self):
        other = self.record.copy()
        other["claim_start_byte"], other["claim_end_byte"] = 12, 24
        other["verdict"] = "not_established"
        self.data["judgments"].append(other)
        self.save_submission()
        self.assertEqual(self.verify(), 0)

    def test_missing_claims_not_certified(self):
        self.data["judgments"] = []
        self.save_submission()
        self.assertEqual(self.verify(report=True), 1)
        self.assertFalse(self.report.exists())

    def test_foreign_reviewer_and_packet_pin_refused(self):
        for key, value in (("reviewer_id", "outsider"), ("packet_sha256", "0" * 64)):
            with self.subTest(field=key):
                original = self.data[key]
                self.data[key] = value
                self.save_submission()
                self.assertEqual(self.verify(), 1)
                self.data[key] = original

    def test_packet_or_upstream_worklist_byte_drift_refused(self):
        original = self.packet.read_bytes()
        self.packet.write_bytes(original + b" ")
        self.assertEqual(self.verify(), 1)
        self.packet.write_bytes(original)
        path = self.paths["output"]
        original = path.read_bytes()
        path.write_bytes(original + b" ")
        self.assertEqual(self.verify(), 1)
        path.write_bytes(original)
        self.assertEqual(self.verify(), 0)

    def test_substituted_revision_or_unassigned_article_refused(self):
        for key, value in (("revision_id", self.record["revision_id"] + 1),
                           ("page_id", 9223372036854775807)):
            with self.subTest(field=key):
                old = self.data["judgments"][0][key]
                self.data["judgments"][0][key] = value
                self.save_submission()
                self.assertEqual(self.verify(), 1)
                self.data["judgments"][0][key] = old

    def test_duplicate_claim_and_conflicting_case_metadata_refused(self):
        self.data["judgments"].append(self.record.copy())
        self.save_submission()
        self.assertEqual(self.verify(), 1)
        self.data["judgments"][1]["claim_start_byte"] = 12
        self.data["judgments"][1]["claim_end_byte"] = 24
        self.data["judgments"][1]["answer_sha256"] = "d" * 64
        self.save_submission()
        self.assertEqual(self.verify(), 1)

    def test_source_digest_conflict_refused(self):
        row = self.record.copy()
        row["case_id"] = "case_2"
        row["source_artifact_sha256"] = "c" * 64
        self.data["judgments"].append(row)
        self.save_submission()
        self.assertEqual(self.verify(), 1)

    def test_malformed_spans_verdict_time_and_flags_refused(self):
        for key, value in (("claim_start_byte", True), ("claim_end_byte", 0),
                           ("verdict", "passed"), ("citation_valid", 1),
                           ("reviewed_at", "2026-11-10T06:00:00"),
                           ("answer_sha256", "G" * 64), ("rubric_version", ""),
                           ("source_span", "")):
            with self.subTest(field=key):
                previous = self.data["judgments"][0][key]
                self.data["judgments"][0][key] = value
                self.save_submission()
                self.assertEqual(self.verify(), 1)
                self.data["judgments"][0][key] = previous

    def test_unexpected_peer_identity_or_completion_claim_refused(self):
        for key, value in (("assigned_reviewer_ids", ["reviewer_b"]),
                           ("review_complete", True)):
            with self.subTest(key=key):
                self.data["judgments"][0][key] = value
                self.save_submission()
                self.assertEqual(self.verify(), 1)
                del self.data["judgments"][0][key]

    def test_nested_duplicate_key_refused(self):
        raw = self.submission.read_text()
        needle = '"verdict": "supported"'
        self.assertIn(needle, raw)
        self.submission.write_text(raw.replace(needle, needle + ', "verdict": "supported"', 1))
        self.assertEqual(self.verify(report=True), 1)
        self.assertFalse(self.report.exists())

    def test_existing_report_cannot_be_overwritten(self):
        self.report.write_text("untouched", encoding="utf-8")
        self.assertEqual(self.verify(report=True), 1)
        self.assertEqual(self.report.read_text(), "untouched")

    @unittest.skipUnless(os.name == "posix", "POSIX file mode required")
    def test_receipt_private_even_with_permissive_umask(self):
        saved_umask = os.umask(0)
        try:
            self.assertEqual(self.verify(report=True), 0)
        finally:
            os.umask(saved_umask)
        self.assertEqual(stat.S_IMODE(self.report.stat().st_mode), 0o600)


if __name__ == "__main__":
    unittest.main()
