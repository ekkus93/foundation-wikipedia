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

    def test_overlapping_claim_spans_refused(self):
        other = self.record.copy()
        other["claim_start_byte"], other["claim_end_byte"] = 5, 18
        self.data["judgments"].append(other)
        self.save_submission()
        self.assertEqual(self.verify(), 1)

    def test_out_of_order_adjacent_claims_allowed(self):
        second = self.record.copy()
        second["claim_start_byte"], second["claim_end_byte"] = 12, 24
        self.data["judgments"].insert(0, second)
        self.save_submission()
        self.assertEqual(self.verify(), 0)

    def test_nonadjacent_overlap_detected_after_sorting(self):
        for start, end in ((0, 3), (12, 16), (3, 12), (15, 19)):
            item = self.record.copy()
            item["claim_start_byte"], item["claim_end_byte"] = start, end
            self.data["judgments"].append(item)
        self.data["judgments"].pop(0)
        self.save_submission()
        self.assertEqual(self.verify(), 1)

    def test_many_disjoint_claims(self):
        self.data["judgments"] = []
        for i in range(2000):
            item = self.record.copy()
            item["claim_start_byte"] = i * 3
            item["claim_end_byte"] = i * 3 + 2
            self.data["judgments"].append(item)
        self.save_submission()
        self.assertEqual(self.verify(), 0)

    def test_source_digest_conflict_refused(self):
        row = self.record.copy()
        row["case_id"] = "case_2"
        row["source_artifact_sha256"] = "c" * 64
        self.data["judgments"].append(row)
        self.save_submission()
        self.assertEqual(self.verify(), 1)

    def test_future_review_timestamp_refused_without_private_receipt(self):
        self.data["judgments"][0]["reviewed_at"] = "2999-01-01T00:00:00+00:00"
        self.save_submission()
        self.assertEqual(self.verify(report=True), 1)
        self.assertFalse(self.report.exists())

    def test_future_review_timestamp_with_offset_refused(self):
        self.data["judgments"][0]["reviewed_at"] = "2998-12-31T16:00:00-08:00"
        self.save_submission()
        self.assertEqual(self.verify(), 1)

    def test_impossible_calendar_review_timestamp_refused(self):
        self.data["judgments"][0]["reviewed_at"] = "2026-02-30T12:00:00Z"
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

    def test_spoofed_or_nonlexical_review_text_rejected(self):
        for field, value in (
                ("source_span", "..."),
                ("source_span", "A\u202eB"),
                ("source_span", "A\u200bB"),
                ("source_span", "A\nB"),
                ("source_span", "A" * 4097),
                ("source_span", "é" * 2049),
                ("source_span", " A cited source sentence"),
                ("rubric_version", "v1\u202e"),
                ("rubric_version", "v1\n"),
                ("rubric_version", "é" * 33),
                ("rubric_version", "...")):
            with self.subTest(field=field, value=value[:20]):
                self.data["judgments"][0][field] = value
                self.save_submission()
                self.assertEqual(self.verify(), 1)
                self.data["judgments"][0][field] = self.record[field]

    def test_multilingual_visible_review_text_accepted_structurally(self):
        self.data["judgments"][0]["source_span"] = "المصدر يحتوي على معلومات"
        self.data["judgments"][0]["rubric_version"] = "نسخة١"
        self.save_submission()
        self.assertEqual(self.verify(), 0)

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

    def strict_fixture(self):
        root = self.paths["output"].parent / "evidence"
        root.mkdir(exist_ok=True)
        (root / "sources").mkdir(exist_ok=True)
        (root / "answers").mkdir(exist_ok=True)
        source_bytes = b"Source text about an astronomical object. A cited source sentence."
        answer_bytes = "The Sun is a star. ⭐".encode("utf-8")
        (root / "sources" / "revision.txt").write_bytes(source_bytes)
        (root / "answers" / "answer.txt").write_bytes(answer_bytes)
        item = self.data["judgments"][0]
        item["source_artifact_sha256"] = hashlib.sha256(source_bytes).hexdigest()
        item["answer_sha256"] = hashlib.sha256(answer_bytes).hexdigest()
        self.save_submission()
        self.evidence_root = root
        self.evidence_map = root.parent / "evidence-map.json"
        self.artifacts = {"schema_version": 1, "sources": [{
            "project": item["project"], "page_id": item["page_id"],
            "revision_id": item["revision_id"], "path": "sources/revision.txt"
        }], "answers": [{"case_id": item["case_id"], "path": "answers/answer.txt"}]}
        self.save_map()

    def save_map(self):
        self.evidence_map.write_text(json.dumps(self.artifacts), encoding="utf-8")

    def strict_verify(self, report=False):
        args = self.base_args + ["--packet", str(self.packet),
                                 "--submission", str(self.submission),
                                 "--evidence-root", str(self.evidence_root),
                                 "--artifact-map", str(self.evidence_map)]
        if report:
            args += ["--report", str(self.report)]
        return submit_main(args)

    def test_strict_local_bytes_and_claim_span_receipt_still_not_qualified(self):
        self.strict_fixture()
        self.assertEqual(self.strict_verify(report=True), 0)
        data = json.loads(self.report.read_text())
        self.assertTrue(data["source_artifact_hashes_verified"])
        self.assertTrue(data["answer_artifact_hashes_verified"])
        self.assertTrue(data["claim_byte_spans_verified"])
        self.assertEqual(data["verification_scope"],
                         "structure-assignment-and-local-evidence-bytes")
        self.assertEqual(data["qualification_status"], "not_qualified")
        self.assertFalse(data["reviewer_identity_authenticated"])
        self.assertFalse(data["independent_human_review_certified"])

    def test_changed_source_or_answer_bytes_rejected_without_report(self):
        for path in ("sources/revision.txt", "answers/answer.txt"):
            with self.subTest(path=path):
                self.strict_fixture()
                target = self.evidence_root / path
                target.write_bytes(target.read_bytes() + b" tamper")
                self.assertEqual(self.strict_verify(report=True), 1)
                self.assertFalse(self.report.exists())

    def test_source_quotation_crosses_streaming_chunk_boundary(self):
        self.strict_fixture()
        source = self.evidence_root / "sources" / "revision.txt"
        raw = b"x" * (65536 - 4) + b"target phrase" + b"y" * 50
        source.write_bytes(raw)
        self.data["judgments"][0]["source_artifact_sha256"] = hashlib.sha256(raw).hexdigest()
        self.data["judgments"][0]["source_span"] = "target phrase"
        self.save_submission()
        self.assertEqual(self.strict_verify(), 0)
        self.data["judgments"][0]["source_span"] = "target phrasE"
        self.save_submission()
        self.assertEqual(self.strict_verify(), 1)


    def test_strict_source_rejects_invalid_utf8_even_when_quote_and_hash_match(self):
        self.strict_fixture()
        source = self.evidence_root / "sources" / "revision.txt"
        raw = b"A cited source sentence.\xff"
        source.write_bytes(raw)
        self.data["judgments"][0]["source_artifact_sha256"] = hashlib.sha256(raw).hexdigest()
        self.save_submission()
        self.assertEqual(self.strict_verify(report=True), 1)
        self.assertFalse(self.report.exists())

    def test_strict_source_accepts_multibyte_utf8_split_across_blocks(self):
        self.strict_fixture()
        source = self.evidence_root / "sources" / "revision.txt"
        raw = b"x" * (65536 - 1) + "é".encode("utf-8") + b" A cited source sentence."
        source.write_bytes(raw)
        self.data["judgments"][0]["source_artifact_sha256"] = hashlib.sha256(raw).hexdigest()
        self.save_submission()
        self.assertEqual(self.strict_verify(), 0)

    def test_strict_source_rejects_truncated_utf8_sequence_at_eof(self):
        self.strict_fixture()
        source = self.evidence_root / "sources" / "revision.txt"
        raw = b"A cited source sentence.\xe2\x82"
        source.write_bytes(raw)
        self.data["judgments"][0]["source_artifact_sha256"] = hashlib.sha256(raw).hexdigest()
        self.save_submission()
        self.assertEqual(self.strict_verify(), 1)

    def test_multiple_source_quotations_same_revision(self):
        self.strict_fixture()
        second = self.data["judgments"][0].copy()
        second["claim_start_byte"], second["claim_end_byte"] = 12, 18
        second["source_span"] = "astronomical object"
        self.data["judgments"].append(second)
        self.save_submission()
        self.assertEqual(self.strict_verify(), 0)
        second["source_span"] = "missing source quotation"
        self.save_submission()
        self.assertEqual(self.strict_verify(), 1)

    def test_source_span_must_occur_verbatim_in_source_file(self):
        self.strict_fixture()
        self.data["judgments"][0]["source_span"] = "Unrelated invented citation"
        self.save_submission()
        self.assertEqual(self.strict_verify(report=True), 1)
        self.assertFalse(self.report.exists())


    def test_strict_multiple_cases_validate_separately(self):
        self.strict_fixture()
        second = self.data["judgments"][0].copy()
        second["case_id"] = "case_2"
        second["answer_sha256"] = hashlib.sha256(b"Another valid answer.").hexdigest()
        second["claim_start_byte"], second["claim_end_byte"] = 0, 7
        self.data["judgments"].append(second)
        (self.evidence_root / "answers" / "answer_2.txt").write_bytes(b"Another valid answer.")
        self.artifacts["answers"].append({"case_id": "case_2", "path": "answers/answer_2.txt"})
        self.save_submission()
        self.save_map()
        self.assertEqual(self.strict_verify(), 0)
        second["claim_end_byte"] = 200
        self.save_submission()
        self.assertEqual(self.strict_verify(report=True), 1)
        self.assertFalse(self.report.exists())

    def test_claim_out_of_bounds_rejected_after_matching_answer_hash(self):
        self.strict_fixture()
        self.data["judgments"][0]["claim_start_byte"] = 20
        self.data["judgments"][0]["claim_end_byte"] = 50
        self.save_submission()
        self.assertEqual(self.strict_verify(), 1)

    def test_mid_utf8_character_boundary_is_rejected(self):
        self.strict_fixture()
        raw = (self.evidence_root / "answers/answer.txt").read_bytes()
        beginning = raw.index("⭐".encode("utf-8"))
        self.data["judgments"][0]["claim_start_byte"] = beginning + 1
        self.data["judgments"][0]["claim_end_byte"] = beginning + 4
        self.save_submission()
        self.assertEqual(self.strict_verify(), 1)

    def test_nonlexical_answer_span_rejected(self):
        self.strict_fixture()
        # Only spaces in the answer substring, despite a valid SHA-256.
        self.data["judgments"][0]["claim_start_byte"] = 3
        self.data["judgments"][0]["claim_end_byte"] = 4
        self.save_submission()
        self.assertEqual(self.strict_verify(), 1)

    def test_strict_rejects_invisible_or_control_evidence_paths(self):
        self.strict_fixture()
        for path in ("sources/revision\u202etxt", "sources/revision\u200b.txt",
                     "sources/revision\n.txt", "sources/revision\x7f.txt"):
            with self.subTest(path=repr(path)):
                self.artifacts["sources"][0]["path"] = path
                self.save_map()
                self.assertEqual(self.strict_verify(report=True), 1)
                self.assertFalse(self.report.exists())

    def test_evidence_map_scope_is_exact(self):
        self.strict_fixture()
        self.artifacts["answers"].append({"case_id": "unreviewed", "path": "answers/answer.txt"})
        self.save_map()
        self.assertEqual(self.strict_verify(), 1)
        self.artifacts["answers"].pop()
        self.artifacts["sources"].clear()
        self.save_map()
        self.assertEqual(self.strict_verify(), 1)

    def test_duplicate_evidence_map_keys_rejected(self):
        self.strict_fixture()
        self.evidence_map.write_text(
            self.evidence_map.read_text().replace('"schema_version": 1',
                                                  '"schema_version": 1, "schema_version": 1'))
        self.assertEqual(self.strict_verify(), 1)

    @unittest.skipUnless(os.name == "posix", "POSIX secure openat required")
    def test_symlink_and_parent_traversal_rejected(self):
        self.strict_fixture()
        path = self.evidence_root / "sources" / "revision.txt"
        path.rename(path.with_suffix(".original"))
        path.symlink_to(path.with_suffix(".original"))
        self.assertEqual(self.strict_verify(), 1)
        path.unlink()
        path.with_suffix(".original").rename(path)
        self.artifacts["sources"][0]["path"] = "../submission.json"
        self.save_map()
        self.assertEqual(self.strict_verify(), 1)

    @unittest.skipUnless(os.name == "posix", "POSIX inode checks required")
    def test_hardlinked_answer_source_alias_rejected(self):
        self.strict_fixture()
        source = self.evidence_root / "sources" / "revision.txt"
        answer = self.evidence_root / "answers" / "answer.txt"
        answer.unlink()
        os.link(source, answer)
        item = self.data["judgments"][0]
        item["answer_sha256"] = item["source_artifact_sha256"]
        self.save_submission()
        self.assertEqual(self.strict_verify(), 1)

    def test_strict_flag_pair_required_and_no_receipt(self):
        self.strict_fixture()
        opts = self.base_args + ["--packet", str(self.packet),
                                 "--submission", str(self.submission),
                                 "--report", str(self.report)]
        self.assertEqual(submit_main(opts + ["--evidence-root", str(self.evidence_root)]), 1)
        self.assertEqual(submit_main(opts + ["--artifact-map", str(self.evidence_map)]), 1)
        self.assertFalse(self.report.exists())


if __name__ == "__main__":
    unittest.main()
