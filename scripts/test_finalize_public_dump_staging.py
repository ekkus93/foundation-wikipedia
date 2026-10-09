"""Offline completeness and integrity tests for public dump staging manifests."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from download_public_dump import DownloadError
from finalize_public_dump_staging import finalize_public_dump, save_manifest
from verify_source_staging import verify_source_bytes

BODY = b"public dump member"
URL = "https://dumps.wikimedia.org/enwiki/20261009/"


class FinalizePublicDumpTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.staging = self.root / "staging"
        self.staging.mkdir()
        (self.staging / "part-a.xml").write_bytes(BODY)
        (self.staging / "part-b.xml").write_bytes(BODY + b"b")
        self.report = {
            "project": "enwiki", "generation_id": "20261009", "completed": True,
            "status_url": URL + "dumpstatus.json",
            "files": [
                {
                    "name": name, "url": URL + name, "bytes": len(payload),
                    "sha1": hashlib.sha1(payload).hexdigest(),
                }
                for name, payload in [
                    ("part-b.xml", BODY + b"b"), ("part-a.xml", BODY)
                ]
            ],
        }

    def finalize(self):
        return finalize_public_dump(self.report, self.staging)

    def test_complete_staging_roundtrips_through_verifier(self):
        manifest = self.finalize()
        self.assertEqual([f["name"] for f in manifest["files"]],
                         ["part-a.xml", "part-b.xml"])
        self.assertEqual(verify_source_bytes(manifest, self.staging), 2)
        output = self.root / "manifest.json"
        save_manifest(manifest, output)
        self.assertEqual(json.loads(output.read_text()), manifest)
        with self.assertRaisesRegex(DownloadError, "already exists"):
            save_manifest(manifest, output)

    def test_missing_truncated_or_corrupted_member_fails_closed(self):
        member = self.staging / "part-a.xml"
        member.unlink()
        with self.assertRaisesRegex(DownloadError, "missing staged"):
            self.finalize()
        member.write_bytes(BODY[:-1])
        with self.assertRaisesRegex(DownloadError, "size mismatch"):
            self.finalize()
        member.write_bytes(b"X" + BODY[1:])
        with self.assertRaisesRegex(DownloadError, "SHA-1 mismatch"):
            self.finalize()

    def test_incomplete_or_forged_report_and_duplicate_members_fail(self):
        self.report["completed"] = False
        with self.assertRaises(DownloadError):
            self.finalize()
        self.report["completed"] = True
        self.report["status_url"] = "https://evil.example/status.json"
        with self.assertRaisesRegex(DownloadError, "status URL"):
            self.finalize()
        self.report["status_url"] = URL + "dumpstatus.json"
        self.report["files"].append(dict(self.report["files"][0]))
        with self.assertRaisesRegex(DownloadError, "duplicate"):
            self.finalize()

    def test_symlink_staging_member_is_not_accepted(self):
        member = self.staging / "part-a.xml"
        member.unlink()
        member.symlink_to(self.root / "missing")
        with self.assertRaises(DownloadError):
            self.finalize()


if __name__ == "__main__":
    unittest.main()
