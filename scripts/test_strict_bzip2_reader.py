"""Verify strict streaming bzip2 handling through the census publication boundary."""
import bz2
import hashlib
from pathlib import Path
import tempfile
import unittest

from public_dump_xml_census import DumpDecodeError, stage_observed_census


def fixture():
    return (b"<mediawiki><page><title>Test</title><ns>0</ns><id>1</id>"
            b"<revision><id>2</id><timestamp>2026-10-01T00:00:00Z</timestamp>"
            b"<text>Hello</text></revision></page></mediawiki>")


class StrictCompressedMemberTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        root = Path(self.temp.name)
        self.staging = root / "staging"
        self.output = root / "private"
        self.staging.mkdir()
        self.output.mkdir(mode=0o700)
        self.name = "enwiki-2026-10-01-p1p100.xml.bz2"

    def stage(self, compressed, output_format="ids"):
        (self.staging / self.name).write_bytes(compressed)
        manifest = {
            "completed": True, "project": "enwiki",
            "generation_id": "2026-10-01",
            "source_url": "https://dumps.wikimedia.org/other/mediawiki_content_current/enwiki/2026-10-01/xml/bzip2/SHA256SUMS",
            "files": [{
                "name": self.name, "sha256": hashlib.sha256(compressed).hexdigest(),
                "bytes": len(compressed),
            }],
        }
        return stage_observed_census(manifest, self.staging, self.output,
                                     "observed.ids", output_format=output_format)

    def test_concatenated_valid_streams_publish_all_formats(self):
        raw = fixture()
        midpoint = len(raw) // 2
        encoded = bz2.compress(raw[:midpoint]) + bz2.compress(raw[midpoint:])
        for output_format in ("ids", "revision-jsonl", "revision-content-jsonl"):
            with self.subTest(output_format=output_format):
                output, count = self.stage(encoded, output_format)
                self.assertEqual(count, 1)
                self.assertTrue(output.read_bytes())
                output.unlink()

    def test_digest_pinned_trailing_garbage_never_publishes(self):
        raw = bz2.compress(fixture())
        for compressed in (raw + b"garbage", raw + b"B",
                           raw + bz2.compress(b"junk"), raw[:-6]):
            for output_format in ("ids", "revision-jsonl", "revision-content-jsonl"):
                with self.subTest(size=len(compressed), output_format=output_format):
                    with self.assertRaises(DumpDecodeError):
                        self.stage(compressed, output_format)
                    self.assertEqual(list(self.output.iterdir()), [])


if __name__ == "__main__":
    unittest.main()
