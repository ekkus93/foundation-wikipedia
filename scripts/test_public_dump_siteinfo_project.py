"""Regression coverage for optional public dump siteinfo database identity."""
import bz2
import hashlib
from pathlib import Path
import tempfile
import unittest

from public_dump_xml_census import DumpDecodeError, stage_observed_census


class SiteinfoProjectTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        root = Path(self.temp.name)
        self.staging = root / "staging"
        self.output = root / "private"
        self.staging.mkdir()
        self.output.mkdir(mode=0o700)

    def run_member(self, siteinfo, generation="2026-10-01"):
        xml = (
            f"<mediawiki>{siteinfo}"
            "<page><title>Page 1</title><ns>0</ns><id>1</id>"
            "<revision><id>301</id><timestamp>2026-10-01T00:00:00Z</timestamp>"
            "<text>Sample</text></revision></page></mediawiki>"
        )
        compressed = bz2.compress(xml.encode("utf-8"))
        name = "enwiki-2026-10-01-p1p100.xml.bz2"
        (self.staging / name).write_bytes(compressed)
        manifest = {
            "completed": True,
            "project": "enwiki",
            "generation_id": generation,
            "source_url": (
                "https://dumps.wikimedia.org/other/mediawiki_content_current/"
                "enwiki/2026-10-01/xml/bzip2/SHA256SUMS"
            ),
            "files": [{
                "name": name,
                "sha256": hashlib.sha256(compressed).hexdigest(),
                "bytes": len(compressed),
            }],
        }
        return stage_observed_census(
            manifest, self.staging, self.output, "observed.ids"
        )

    def test_matching_project_and_legacy_missing_dbname(self):
        for siteinfo in (
            "<siteinfo><dbname>enwiki</dbname></siteinfo>",
            "<siteinfo><sitename>Wikipedia</sitename></siteinfo>",
            "",
        ):
            with self.subTest(siteinfo=siteinfo):
                path, count = self.run_member(siteinfo)
                self.assertEqual(count, 1)
                self.assertEqual(path.read_bytes(), b"1\n")
                path.unlink()

    def test_invalid_calendar_generation_fails_closed(self):
        for generation in ("2026-02-30", "2026-13-01", "0000-01-01", "2026-1-01"):
            with self.subTest(generation=generation):
                with self.assertRaisesRegex(DumpDecodeError, "generation calendar date"):
                    self.run_member("", generation=generation)
                self.assertEqual(list(self.output.iterdir()), [])

    def test_mismatch_duplicate_and_foreign_dbname_fail_closed(self):
        for siteinfo in (
            "<siteinfo><dbname>dewiki</dbname></siteinfo>",
            "<siteinfo><dbname>enwiki</dbname><dbname>enwiki</dbname></siteinfo>",
            '<siteinfo xmlns:foreign="urn:foreign">'
            "<foreign:dbname>enwiki</foreign:dbname></siteinfo>",
            "<siteinfo><dbname><nested>enwiki</nested></dbname></siteinfo>",
        ):
            with self.subTest(siteinfo=siteinfo):
                with self.assertRaisesRegex(DumpDecodeError, "siteinfo database name"):
                    self.run_member(siteinfo)
                self.assertEqual(list(self.output.iterdir()), [])


if __name__ == "__main__":
    unittest.main()
