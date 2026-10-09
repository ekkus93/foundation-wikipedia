"""Negative and positive cases for fixture provenance validation."""
import hashlib
import json
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from verify_fixture_provenance import verify


class FixtureProvenanceEdgeTests(unittest.TestCase):
    def setUp(self):
        self.temp = TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.fixture = self.root / "article.txt"
        self.fixture.write_bytes(b"fixture bytes")

    def sidecar(self, **changes):
        metadata = {
            "source": "synthetic",
            "description": "Locally generated deterministic fixture",
            "license": "CC0-1.0",
            "sha256": hashlib.sha256(self.fixture.read_bytes()).hexdigest(),
        }
        metadata.update(changes)
        (self.root / "article.txt.provenance.json").write_text(
            json.dumps(metadata), encoding="utf-8"
        )

    def assert_error(self, fragment):
        self.assertTrue(
            any(fragment in error for error in verify(self.root)),
            f"Expected {fragment!r} in {verify(self.root)!r}",
        )

    def test_valid_synthetic_fixture(self):
        self.sidecar()
        self.assertEqual(verify(self.root), [])

    def test_missing_directory_fails_closed(self):
        self.assertIn("missing or unsafe", " ".join(verify(self.root / "missing")))

    def test_missing_sidecar(self):
        self.assert_error("missing provenance sidecar")

    def test_orphan_sidecar(self):
        self.sidecar()
        self.fixture.unlink()
        self.assert_error("orphan provenance sidecar")

    def test_mismatched_bytes(self):
        self.sidecar()
        self.fixture.write_bytes(b"tampered")
        self.assert_error("sha256 mismatch")

    def test_invalid_license_and_description(self):
        self.sidecar(license=" ", description="")
        self.assert_error("missing license")
        self.assert_error("missing description")

    def test_invalid_source_and_hash(self):
        self.sidecar(source="unknown", sha256="not a hash")
        self.assert_error("invalid source")
        self.assert_error("invalid sha256")

    def test_valid_wikimedia_fixture(self):
        self.sidecar(
            source="wikimedia", source_url="https://dumps.wikimedia.org/enwiki/",
            project="enwiki", page_id=1, revision_id=2,
        )
        self.assertEqual(verify(self.root), [])

    def test_wikimedia_rejects_invalid_revision_and_url(self):
        self.sidecar(
            source="wikimedia", source_url="http://example.org",
            project="en-wiki", page_id=True, revision_id=0,
        )
        self.assert_error("missing HTTPS source_url")
        self.assert_error("invalid project")
        self.assert_error("invalid page_id")
        self.assert_error("invalid revision_id")

    def test_wikimedia_source_rejects_lookalike_hosts(self):
        for url in (
            "https://example.org/enwiki/",
            "https://dumps.wikimedia.org.attacker.example/enwiki/",
            "https://wikimedia.org@attacker.example/enwiki/",
            "https://dumps.wikimedia.org:8443/enwiki/",
            "https://dumps.wikimedia.org.evil.org/enwiki/",
        ):
            with self.subTest(url=url):
                self.sidecar(
                    source="wikimedia", source_url=url,
                    project="enwiki", page_id=1, revision_id=2,
                )
                self.assert_error("missing HTTPS source_url")

    def test_empty_fixture_directory_fails_closed(self):
        self.fixture.unlink()
        self.assert_error("no fixture data files")

    def test_readme_only_fixture_directory_fails_closed(self):
        self.fixture.unlink()
        (self.root / "README.md").write_text("fixture guidance", encoding="utf-8")
        self.assert_error("no fixture data files")

    def test_directory_symlink_is_rejected(self):
        target = self.root / "actual"
        target.mkdir()
        (self.root / "linked").symlink_to(target, target_is_directory=True)
        self.assert_error("symlink not allowed")


if __name__ == "__main__":
    unittest.main()
