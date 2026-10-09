"""Offline tests for local-file dump staging and no-clobber guarantees."""
import hashlib
from pathlib import Path
import tempfile
import unittest

from download_public_dump import DownloadError
from stage_local_dump import stage_local_dump_member

BODY = b"offline wikipedia dump bytes"
URL = "https://dumps.wikimedia.org/enwiki/20261009/articles.xml"


class LocalDumpStagingTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        self.source = root / "download.xml"
        self.source.write_bytes(BODY)
        self.destination = root / "staging"
        self.report = {
            "project": "enwiki", "generation_id": "20261009", "completed": True,
            "files": [{
                "name": "articles.xml", "url": URL, "bytes": len(BODY),
                "sha1": hashlib.sha1(BODY).hexdigest(),
            }],
        }

    def stage(self):
        return stage_local_dump_member(
            self.report, "articles.xml", self.source, self.destination
        )

    def test_valid_local_file_promoted_with_both_digest_receipts(self):
        receipt = self.stage()
        self.assertEqual(receipt["sha256"], hashlib.sha256(BODY).hexdigest())
        self.assertEqual((self.destination / "articles.xml").read_bytes(), BODY)
        self.assertEqual(self.stage(), receipt)
        self.assertEqual(list(self.destination.glob("*.local-part")), [])

    def test_rejects_corruption_and_wrong_size_without_final_member(self):
        self.source.write_bytes(b"X" + BODY[1:])
        with self.assertRaisesRegex(DownloadError, "SHA-1 mismatch"):
            self.stage()
        self.assertFalse((self.destination / "articles.xml").exists())
        self.source.write_bytes(BODY[:-1])
        with self.assertRaisesRegex(DownloadError, "size mismatch"):
            self.stage()
        self.assertFalse((self.destination / "articles.xml").exists())

    def test_preserves_http_partial_and_rejects_bad_existing_final(self):
        self.destination.mkdir()
        (self.destination / "articles.xml.part").write_bytes(BODY[:4])
        self.stage()
        self.assertEqual((self.destination / "articles.xml.part").read_bytes(), BODY[:4])
        (self.destination / "articles.xml").write_bytes(b"X" + BODY[1:])
        with self.assertRaisesRegex(DownloadError, "existing final member SHA-1 mismatch"):
            self.stage()

    def test_rejects_symlink_source_and_destination(self):
        link = self.source.parent / "link.xml"
        link.symlink_to(self.source)
        with self.assertRaisesRegex(DownloadError, "unsafe existing"):
            stage_local_dump_member(self.report, "articles.xml", link, self.destination)
        self.destination.symlink_to(self.source.parent, target_is_directory=True)
        with self.assertRaisesRegex(DownloadError, "staging destination is a symlink"):
            self.stage()

    def test_rejects_incomplete_or_forged_discovery_report(self):
        self.report["completed"] = False
        with self.assertRaisesRegex(DownloadError, "incomplete"):
            self.stage()
        self.report["completed"] = True
        self.report["files"][0]["url"] = "https://attacker.example/file"
        with self.assertRaisesRegex(DownloadError, "official URL"):
            self.stage()


if __name__ == "__main__":
    unittest.main()
