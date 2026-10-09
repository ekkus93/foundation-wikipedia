"""Checks both matching staged source bytes and adversarial failure modes."""
import copy
import hashlib
import tempfile
import unittest
from pathlib import Path

from verify_source_staging import SourceVerificationError, verify_source_bytes


class SourceStagingTests(unittest.TestCase):
    def setUp(self):
        self.tempdir = tempfile.TemporaryDirectory()
        self.addCleanup(self.tempdir.cleanup)
        self.dir = Path(self.tempdir.name) / "staging"
        self.dir.mkdir()
        self.member = self.dir / "articles.xml"
        self.member.write_bytes(b"<mediawiki>fixed revision</mediawiki>")
        self.manifest = {
            "project": "enwiki",
            "generation_id": "20261009",
            "source_url": "https://dumps.wikimedia.org/enwiki/20261009/dumpstatus.json",
            "completed": True,
            "files": [{
                "name": self.member.name,
                "bytes": self.member.stat().st_size,
                "sha256": hashlib.sha256(self.member.read_bytes()).hexdigest(),
            }],
        }

    def reject(self, phrase):
        with self.assertRaisesRegex(SourceVerificationError, phrase):
            verify_source_bytes(self.manifest, self.dir)

    def test_matching_staged_bytes_pass(self):
        self.assertEqual(verify_source_bytes(self.manifest, self.dir), 1)

    def test_corrupt_or_truncated_bytes_fail_closed(self):
        self.member.write_bytes(b"changed")
        self.reject("byte length mismatch")
        self.manifest["files"][0]["bytes"] = self.member.stat().st_size
        self.reject("sha256 mismatch")

    def test_incomplete_or_untrusted_origin_rejected(self):
        self.manifest["completed"] = False
        self.reject("generation metadata")
        self.manifest["completed"] = True
        for url in [
            "http://dumps.wikimedia.org/enwiki/",
            "https://dumps.wikimedia.org.attacker.example/enwiki/",
            "https://dumps.wikimedia.org@attacker.example/enwiki/",
        ]:
            self.manifest["source_url"] = url
            self.reject("generation metadata")

    def test_dump_status_url_is_bound_to_exact_project_and_generation(self):
        accepted = self.manifest["source_url"]
        for forgery in [
            "https://dumps.wikimedia.org/frwiki/20261009/dumpstatus.json",
            "https://dumps.wikimedia.org/enwiki/20261008/dumpstatus.json",
            "https://dumps.wikimedia.org/enwiki/20261009/",
            "https://dumps.wikimedia.org/enwiki/20261009/dumpstatus.json?alt=1",
            "https://dumps.wikimedia.org/enwiki/20261009/dumpstatus.json#alternate",
            "https://dumps.wikimedia.org/enwiki/20261009/../20261009/dumpstatus.json",
        ]:
            self.manifest["source_url"] = forgery
            self.reject("generation metadata")
        self.manifest["source_url"] = accepted
        self.assertEqual(verify_source_bytes(self.manifest, self.dir), 1)

    def test_bad_ids_names_and_duplicate_case_aliases(self):
        self.manifest["project"] = "en/wiki"
        self.reject("generation metadata")
        self.manifest["project"] = "enwiki"
        for name in ["../escape", "CON.xml", "COM1", "bad\\path", "hidden.", ""]:
            self.manifest["files"][0]["name"] = name
            self.reject("unsafe or incomplete")
        self.manifest["files"][0]["name"] = self.member.name
        second = copy.deepcopy(self.manifest["files"][0])
        second["name"] = "ARTICLES.XML"
        self.manifest["files"].append(second)
        self.reject("duplicate or case-colliding")

    def test_unsafe_members_and_invalid_metadata(self):
        self.member.unlink()
        self.member.symlink_to(Path(self.tempdir.name) / "outside")
        self.reject("missing or unsafe")
        self.member.unlink()
        self.member.write_bytes(b"data")
        self.manifest["files"][0]["bytes"] = True
        self.reject("unsafe or incomplete")
        self.manifest["files"][0]["bytes"] = 4
        self.manifest["files"][0]["sha256"] = "invalid"
        self.reject("unsafe or incomplete")

    def test_empty_manifest_and_root_symlink_rejected(self):
        self.manifest["files"] = []
        self.reject("bounded nonempty")
        self.manifest["files"] = [dict(name="articles.xml", bytes=30, sha256="0" * 64)]
        alias = Path(self.tempdir.name) / "alias"
        alias.symlink_to(self.dir, target_is_directory=True)
        with self.assertRaisesRegex(SourceVerificationError, "directory"):
            verify_source_bytes(self.manifest, alias)


if __name__ == "__main__":
    unittest.main()
