"""Offline end-to-end staging tests for complete public dump generations."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from download_public_dump import DownloadError
from stage_public_dump_generation import stage_generation
from verify_source_staging import verify_source_bytes

BODY_A = b"article dump a"
BODY_B = b"article dump b"
URL = "https://dumps.wikimedia.org/enwiki/20261009/"


class GenerationStagingTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.local = self.root / "local"
        self.local.mkdir()
        self.staging = self.root / "staging"
        self.manifest = self.root / "manifest.json"
        self.report = {
            "project": "enwiki", "generation_id": "20261009",
            "status_url": URL + "dumpstatus.json", "completed": True,
            "files": [
                {
                    "name": name, "url": URL + name,
                    "bytes": len(body), "sha1": hashlib.sha1(body).hexdigest(),
                }
                for name, body in [
                    ("b.xml", BODY_B), ("a.xml", BODY_A),
                ]
            ],
        }
        (self.local / "a.xml").write_bytes(BODY_A)
        (self.local / "b.xml").write_bytes(BODY_B)

    def stage(self):
        return stage_generation(
            self.report, self.staging, self.manifest,
            local_directory=self.local,
        )

    def test_complete_offline_generation_produces_verified_manifest(self):
        result = self.stage()
        self.assertEqual([f["name"] for f in result["files"]], ["a.xml", "b.xml"])
        self.assertEqual(json.loads(self.manifest.read_text()), result)
        self.assertEqual(verify_source_bytes(result, self.staging), 2)

    def test_missing_member_leaves_no_installable_manifest(self):
        (self.local / "b.xml").unlink()
        with self.assertRaises(DownloadError):
            self.stage()
        self.assertTrue((self.staging / "a.xml").exists())
        self.assertFalse(self.manifest.exists())
        (self.local / "b.xml").write_bytes(BODY_B)
        self.assertEqual(verify_source_bytes(self.stage(), self.staging), 2)

    def test_corrupt_member_leaves_no_manifest(self):
        (self.local / "b.xml").write_bytes(b"X" + BODY_B[1:])
        with self.assertRaises(DownloadError):
            self.stage()
        self.assertFalse(self.manifest.exists())

    def test_bad_inventory_is_rejected_before_staging(self):
        self.report["files"].append(dict(self.report["files"][0]))
        with self.assertRaisesRegex(DownloadError, "duplicate"):
            self.stage()
        self.assertFalse(self.staging.exists())
        self.report["files"].pop()
        self.report["completed"] = False
        with self.assertRaisesRegex(DownloadError, "incomplete"):
            self.stage()
        self.assertFalse(self.staging.exists())

    def test_no_implicit_network_without_user_agent(self):
        with self.assertRaisesRegex(DownloadError, "user-agent"):
            stage_generation(self.report, self.staging, self.manifest)


if __name__ == "__main__":
    unittest.main()
