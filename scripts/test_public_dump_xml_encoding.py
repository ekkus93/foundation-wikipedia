"""UTF-16 DTD regression: raw-byte DTD checks must not be bypassable."""
import bz2
import hashlib
from pathlib import Path
import tempfile
import unittest

from public_dump_xml_census import DumpDecodeError, stage_observed_census


class XmlEncodingGuardTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        root = Path(self.temp.name)
        self.staging = root / "staging"
        self.output = root / "private"
        self.staging.mkdir()
        self.output.mkdir(mode=0o700)
        self.name = "enwiki-2026-10-01-p1p100.xml.bz2"

    def stage(self, raw, output_format="ids"):
        compressed = bz2.compress(raw)
        (self.staging / self.name).write_bytes(compressed)
        manifest = {
            "completed": True,
            "project": "enwiki",
            "generation_id": "2026-10-01",
            "source_url": "https://dumps.wikimedia.org/other/mediawiki_content_current/enwiki/2026-10-01/xml/bzip2/SHA256SUMS",
            "files": [{
                "name": self.name,
                "sha256": hashlib.sha256(compressed).hexdigest(),
                "bytes": len(compressed),
            }],
        }
        return stage_observed_census(
            manifest, self.staging, self.output, "observed.ids",
            output_format=output_format)

    def test_utf16_dtd_cannot_hide_from_raw_byte_guard(self):
        xml = (
            '<?xml version="1.0" encoding="utf-16"?>'
            '<!DOCTYPE mediawiki [<!ENTITY inject "EXPANDED">]>'
            '<mediawiki><page><title>Test</title><ns>0</ns><id>1</id>'
            '<revision><id>2</id><timestamp>2026-10-01T00:00:00Z</timestamp>'
            '<text>&inject;</text></revision></page></mediawiki>'
        )
        raw = xml.encode("utf-16")
        self.assertNotIn(b"<!DOCTYPE", raw)
        for output_format in ("ids", "revision-jsonl", "revision-content-jsonl"):
            with self.subTest(output_format=output_format):
                with self.assertRaisesRegex(DumpDecodeError, "non-UTF-8 XML"):
                    self.stage(raw, output_format)
                self.assertEqual(list(self.output.iterdir()), [])

    def test_utf16_without_dtd_is_rejected_but_utf8_remains_valid(self):
        xml = (
            '<mediawiki><page><title>München</title><ns>0</ns><id>1</id>'
            '<revision><id>2</id><timestamp>2026-10-01T00:00:00Z</timestamp>'
            '<text>Sample</text></revision></page></mediawiki>'
        )
        with self.assertRaisesRegex(DumpDecodeError, "non-UTF-8 XML"):
            self.stage(xml.encode("utf-16"))
        self.assertEqual(list(self.output.iterdir()), [])
        result, count = self.stage(xml.encode("utf-8"))
        self.assertEqual((count, result.read_bytes()), (1, b"1\\n"))


if __name__ == "__main__":
    unittest.main()
