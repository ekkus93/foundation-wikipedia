"""Fail-closed regression tests for the optional diagnostic JSONL audit."""
import hashlib
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from audit_public_dump_revision_jsonl import (
    RevisionAuditError, audit_revision_jsonl,
)


class RevisionJsonlAuditTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name) / "revisions.jsonl"

    def record(self, page_id=1, content=False):
        value = {
            "schema_version": 1, "project": "enwiki",
            "generation_id": "2026-10-01", "page_id": page_id,
            "namespace": 0, "title": "München",
            "revision_id": 200 + page_id,
            "timestamp": "2026-10-01T00:00:00Z",
            "redirect_target": None,
        }
        if content:
            value["wikitext"] = "Grüße & 你好\n== Section =="
            value["wikitext_sha256"] = hashlib.sha256(
                value["wikitext"].encode("utf-8")).hexdigest()
        return value

    def write(self, *records):
        self.path.write_bytes(b"".join(
            (json.dumps(record, ensure_ascii=False, sort_keys=True) + "\n").encode()
            for record in records))

    def audit(self, content=False):
        return audit_revision_jsonl(
            self.path, "enwiki", "2026-10-01", 0, content)

    def test_valid_scoped_metadata_and_content(self):
        self.write(self.record(1), self.record(2))
        count, digest = self.audit()
        self.assertEqual(count, 2)
        self.assertEqual(digest, hashlib.sha256(self.path.read_bytes()).hexdigest())
        self.write(self.record(1, True), self.record(2, True))
        self.assertEqual(self.audit(True)[0], 2)
        with self.assertRaisesRegex(RevisionAuditError, "schema"):
            self.audit(False)

    def test_bad_identity_scope_and_order_fail_closed(self):
        cases = [
            ("schema_version", 2), ("project", "frwiki"),
            ("generation_id", "2026-10-02"), ("namespace", 1),
            ("page_id", 0), ("page_id", 2**63),
            ("revision_id", 0), ("revision_id", True),
            ("title", " Bad "), ("title", "Bad\nTitle"),
            ("timestamp", "2026-02-30T00:00:00Z"),
            ("timestamp", "2026-10-02T00:00:00Z"),
            ("redirect_target", " Bad "),
        ]
        for key, bad in cases:
            with self.subTest(key=key, bad=bad):
                record = self.record()
                record[key] = bad
                self.write(record)
                with self.assertRaises(RevisionAuditError):
                    self.audit()
        self.write(self.record(2), self.record(1))
        with self.assertRaisesRegex(RevisionAuditError, "unordered"):
            self.audit()
        self.write(self.record(1), self.record(1))
        with self.assertRaisesRegex(RevisionAuditError, "unordered"):
            self.audit()

    def test_generation_cutoff_and_lf_contract_for_both_revision_modes(self):
        for content in (False, True):
            with self.subTest(content=content):
                prior = self.record(content=content)
                prior["timestamp"] = "2026-09-30T23:59:59Z"
                self.write(prior)
                self.assertEqual(self.audit(content)[0], 1)

                future = self.record(content=content)
                future["timestamp"] = "2026-10-02T00:00:00Z"
                self.write(future)
                with self.assertRaisesRegex(RevisionAuditError, "timestamp"):
                    self.audit(content)

                self.write(self.record(content=content))
                raw = self.path.read_bytes()
                self.path.write_bytes(raw.replace(b", ", b",\\r ", 1))
                with self.assertRaisesRegex(RevisionAuditError, "unterminated"):
                    self.audit(content)

    def test_invalid_wikitext_digest_and_extra_keys_rejected(self):
        good = self.record(content=True)
        for key, bad in (
            ("wikitext_sha256", "0" * 64),
            ("wikitext_sha256", "ABC"),
            ("wikitext", 12),
            ("wikitext", "different"),
        ):
            record = dict(good)
            record[key] = bad
            self.write(record)
            with self.assertRaises(RevisionAuditError):
                self.audit(True)
        record = self.record()
        record["unexpected"] = "value"
        self.write(record)
        with self.assertRaisesRegex(RevisionAuditError, "schema"):
            self.audit()

    def test_duplicate_json_nonstandard_and_bounded_lines(self):
        self.path.write_bytes(
            b'{"schema_version":1,"schema_version":1}\n')
        with self.assertRaises(RevisionAuditError):
            self.audit()
        self.path.write_bytes(b'{"extra":NaN}\n')
        with self.assertRaises(RevisionAuditError):
            self.audit()
        self.write(self.record())
        with patch("audit_public_dump_revision_jsonl.MAX_RECORD_BYTES", 10):
            with self.assertRaisesRegex(RevisionAuditError, "oversized"):
                self.audit()
        self.path.write_bytes(self.path.read_bytes().rstrip(b"\n"))
        with self.assertRaisesRegex(RevisionAuditError, "unterminated"):
            self.audit()

    def test_missing_symlink_hardlink_and_empty_file(self):
        with self.assertRaises(RevisionAuditError):
            self.audit()
        self.path.write_bytes(b"")
        with self.assertRaises(RevisionAuditError):
            self.audit()
        self.write(self.record())
        alias = self.path.with_name("alias.jsonl")
        alias.symlink_to(self.path)
        with self.assertRaises(RevisionAuditError):
            audit_revision_jsonl(alias, "enwiki", "2026-10-01", 0)
        other = self.path.with_name("hardlink.jsonl")
        os.link(self.path, other)
        with self.assertRaises(RevisionAuditError):
            self.audit()


if __name__ == "__main__":
    unittest.main()
