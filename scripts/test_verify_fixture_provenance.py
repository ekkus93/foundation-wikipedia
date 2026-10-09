"""Negative regression coverage for fail-closed fixture provenance validation."""

import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from verify_fixture_provenance import verify


class ProvenanceTests(unittest.TestCase):
    def test_committed_fixtures(self):
        root = Path(__file__).resolve().parents[1] / "fixtures"
        self.assertEqual(verify(root), [])

    def setUp(self):
        self.tempdir = tempfile.TemporaryDirectory()
        self.addCleanup(self.tempdir.cleanup)
        self.root = Path(self.tempdir.name) / "fixtures"
        self.root.mkdir()
        self.fixture = self.root / "sample.txt"
        self.fixture.write_bytes(b"fixture bytes")
        self.metadata = self.root / "sample.txt.provenance.json"
        self.data = {
            "source": "synthetic",
            "description": "Deterministic test fixture",
            "license": "CC0-1.0",
            "sha256": hashlib.sha256(self.fixture.read_bytes()).hexdigest(),
        }
        self.write_metadata()

    def write_metadata(self):
        self.metadata.write_text(json.dumps(self.data), encoding="utf-8")

    def assert_rejected(self, fragment):
        failures = verify(self.root)
        self.assertTrue(
            any(fragment in failure for failure in failures),
            f"expected {fragment!r} in {failures!r}",
        )

    def test_valid_synthetic_fixture(self):
        self.assertEqual(verify(self.root), [])

    def test_missing_fixture_directory_is_rejected(self):
        self.assert_rejected_when_root_missing()

    def assert_rejected_when_root_missing(self):
        self.assertIn("missing or unsafe fixture directory", " ".join(verify(self.root / "missing")))

    def test_missing_and_orphaned_sidecars_are_rejected(self):
        self.metadata.unlink()
        self.assert_rejected("missing provenance sidecar")
        self.fixture.unlink()
        self.metadata.write_text("{}", encoding="utf-8")
        self.assert_rejected("orphan provenance sidecar")

    def test_malformed_json_and_nonobject_metadata_are_rejected(self):
        self.metadata.write_text("{broken", encoding="utf-8")
        self.assert_rejected("invalid JSON")
        self.metadata.write_text("[]", encoding="utf-8")
        self.assert_rejected("expected object")

    def test_corrupted_bytes_and_invalid_hashes_are_rejected(self):
        self.fixture.write_bytes(b"corrupted")
        self.assert_rejected("sha256 mismatch")
        self.data["sha256"] = "invalid"
        self.write_metadata()
        self.assert_rejected("invalid sha256")

    def test_missing_license_and_description_are_rejected(self):
        self.data["license"] = "  "
        self.write_metadata()
        self.assert_rejected("missing license")
        self.data["license"] = "CC0-1.0"
        self.data["description"] = ""
        self.write_metadata()
        self.assert_rejected("missing description")

    def test_official_source_requires_exact_identity_and_host(self):
        self.data.update({
            "source": "wikimedia",
            "source_url": "https://dumps.wikimedia.org/enwiki/20261001/",
            "project": "enwiki",
            "page_id": 123,
            "revision_id": 456,
        })
        self.assertEqual(verify(self.root), [])
        for url in (
            "http://dumps.wikimedia.org/enwiki/",
            "https://dumps.wikimedia.org.attacker.example/enwiki/",
            "https://wikimedia.org@attacker.example/enwiki/",
            "https://dumps.wikimedia.org:444/enwiki/",
        ):
            with self.subTest(url=url):
                self.data["source_url"] = url
                self.write_metadata()
                self.assert_rejected("missing HTTPS source_url")
        self.data["source_url"] = "https://dumps.wikimedia.org/enwiki/"
        for key, value, fragment in (
            ("project", "en:wiki", "invalid project"),
            ("page_id", 0, "invalid page_id"),
            ("page_id", True, "invalid page_id"),
            ("revision_id", "456", "invalid revision_id"),
        ):
            with self.subTest(key=key, value=value):
                original = self.data[key]
                self.data[key] = value
                self.write_metadata()
                self.assert_rejected(fragment)
                self.data[key] = original

    def test_symlink_fixture_is_rejected(self):
        target = Path(self.tempdir.name) / "outside.txt"
        target.write_text("outside", encoding="utf-8")
        (self.root / "outside.txt").symlink_to(target)
        self.assert_rejected("symlink not allowed")

    def test_empty_fixture_suite_is_rejected(self):
        self.fixture.unlink()
        self.metadata.unlink()
        self.assert_rejected("no fixture data files")


if __name__ == "__main__":
    unittest.main()
