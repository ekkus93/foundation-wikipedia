"""Negative tests for current-content shard staging integrity."""
import hashlib
from pathlib import Path
import tempfile
import unittest

from stage_public_current_shards import stage_verified_members
from test_stage_public_current_shards import FakeOpener


class NegativeStagingTests(unittest.TestCase):
    def test_existing_bytes_must_match_official_inventory(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            stage = root / "stage"
            stage.mkdir()
            inventory = root / "SHA256SUMS"
            member = "enwiki-2026-10-01-p1p100.xml.bz2"
            data = b"expected bytes"
            inventory.write_text(hashlib.sha256(data).hexdigest() + "  " + member + "\n")
            (stage / member).write_bytes(b"other bytes")
            opener = FakeOpener(data)
            with self.assertRaisesRegex(ValueError, "SHA-256 mismatch"):
                stage_verified_members(inventory, "enwiki", "2026-10-01",
                                       stage, opener=opener)
            self.assertEqual(opener.calls, 0)
            self.assertEqual((stage / member).read_bytes(), b"other bytes")

    def test_download_bytes_must_match_official_inventory(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            stage = root / "stage"
            stage.mkdir()
            inventory = root / "SHA256SUMS"
            member = "enwiki-2026-10-01-p1p100.xml.bz2"
            inventory.write_text(hashlib.sha256(b"expected").hexdigest() + "  " + member + "\n")
            with self.assertRaisesRegex(ValueError, "SHA-256 mismatch"):
                stage_verified_members(inventory, "enwiki", "2026-10-01",
                                       stage, opener=FakeOpener(b"other"))
            self.assertFalse((stage / member).exists())
            self.assertFalse(list(stage.glob(".wiki-shard.*")))


if __name__ == "__main__":
    unittest.main()
