"""Offline end-to-end staging fixtures for the completed current-content export."""
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest

from discover_public_dump_inventory import BASE, PublicationError
from stage_public_dump_export import stage_completed_export


class StageTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.target = self.root / "staged"
        self.date = "2026-10-01"
        self.a = f"enwiki-{self.date}-p1p100.xml.bz2"
        self.b = f"enwiki-{self.date}-p101p200.xml.bz2"
        self.payloads = {self.a: b"first synthetic shard",
                         self.b: b"second synthetic shard"}

    def fetch(self, url, limit):
        if url == BASE + "enwiki/":
            return b'<a href="2026-10-01/">Complete</a>'
        if url.endswith("SHA256SUMS"):
            return "".join(
                f"{hashlib.sha256(data).hexdigest()}  {name}\n"
                for name, data in self.payloads.items()
            ).encode()
        raise AssertionError(url)

    def stream(self, url):
        self.assertTrue(url.startswith(BASE + "enwiki/2026-10-01/xml/bzip2/"))
        return io.BytesIO(self.payloads[url.rsplit("/", 1)[-1]])

    def stage(self, **kwargs):
        return stage_completed_export("enwiki", self.target,
                                      fetch=self.fetch, stream=self.stream, **kwargs)

    def test_two_shards_and_inventory_publish_as_one_directory(self):
        receipt = self.stage()
        self.assertEqual(receipt["members"], 2)
        self.assertEqual(receipt["compressed_bytes"], sum(map(len, self.payloads.values())))
        for name, data in self.payloads.items():
            self.assertEqual((self.target / name).read_bytes(), data)
        self.assertEqual(json.loads((self.target / "PUBLICATION.json").read_text()),
                         receipt)
        self.assertIn(self.a, (self.target / "SHA256SUMS").read_text())
        with self.assertRaisesRegex(PublicationError, "already exists"):
            self.stage()

    def test_corrupted_later_member_does_not_publish_partial_directory(self):
        def corrupted(url):
            if url.endswith(self.b):
                return io.BytesIO(b"wrong")
            return self.stream(url)

        with self.assertRaisesRegex(PublicationError, "SHA-256 mismatch"):
            stage_completed_export("enwiki", self.target,
                                   fetch=self.fetch, stream=corrupted)
        self.assertFalse(self.target.exists())
        self.assertFalse(list(self.root.glob(".current-export.*")))

    def test_aggregate_and_member_budgets_fail_closed(self):
        for limits in ({"max_total_bytes": len(self.payloads[self.a])},
                       {"max_member_bytes": 5},
                       {"max_total_bytes": 0}):
            with self.subTest(limits=limits):
                with self.assertRaises(PublicationError):
                    self.stage(**limits)
                self.assertFalse(self.target.exists())
                self.assertFalse(list(self.root.glob(".current-export.*")))


if __name__ == "__main__":
    unittest.main()
