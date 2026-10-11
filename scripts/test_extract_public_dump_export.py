"""Synthetic all-or-nothing current-export extraction acceptance."""
import bz2
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from extract_public_dump_export import extract_current_export, DumpExtractionError


def xml(page_id):
    return (f'<?xml version="1.0" encoding="utf-8"?>'
            f'<mediawiki><page><title>Article {page_id}</title><ns>0</ns>'
            f'<id>{page_id}</id><revision><id>{page_id + 1000}</id>'
            f'<timestamp>2026-10-01T00:00:00Z</timestamp>'
            f'<text>Wiki {page_id}</text></revision></page></mediawiki>').encode()


class ExportTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.stage = self.root / "private"
        self.stage.mkdir()
        self.inventory = self.root / "SHA256SUMS"
        self.output = self.root / "pages.jsonl"
        self.first = "enwiki-2026-10-01-p1p100.xml.bz2"
        self.second = "enwiki-2026-10-01-p101p200.xml.bz2"
        self.shards = {
            self.first: bz2.compress(xml(10)),
            self.second: bz2.compress(xml(110)),
        }
        self.write_inventory()

    def write_inventory(self):
        # Reverse inventory order to require deterministic page-ID ordering.
        for path, data in self.shards.items():
            (self.stage / path).write_bytes(data)
        self.inventory.write_text("".join(
            f"{hashlib.sha256(data).hexdigest()}  {name}\n"
            for name, data in reversed(list(self.shards.items()))
        ))

    def extract(self, **kwargs):
        return extract_current_export(
            self.stage, self.inventory, "enwiki", "2026-10-01", self.output, **kwargs,
        )

    def assert_no_publication(self):
        self.assertFalse(self.output.exists())
        self.assertFalse(list(self.root.glob(".dump-export.*")))

    def test_two_shards_publish_one_ordered_revision_scoped_file(self):
        self.assertEqual(self.extract(), 2)
        entries = [json.loads(line) for line in self.output.read_text().splitlines()]
        self.assertEqual([entry["page_id"] for entry in entries], [10, 110])
        self.assertEqual([entry["wikitext"] for entry in entries], ["Wiki 10", "Wiki 110"])
        self.assertNotEqual(entries[0]["source_sha256"], entries[1]["source_sha256"])
        self.assertFalse(list(self.root.glob(".dump-export.*")))
        with self.assertRaisesRegex(DumpExtractionError, "existing output"):
            self.extract()

    def test_mismatched_or_missing_shard_never_publishes_partial_export(self):
        (self.stage / self.second).write_bytes(b"wrong")
        with self.assertRaisesRegex(DumpExtractionError, "SHA-256 mismatch"):
            self.extract()
        self.assert_no_publication()
        (self.stage / self.second).unlink()
        with self.assertRaises(FileNotFoundError):
            self.extract()
        self.assert_no_publication()

    def test_second_shard_invalid_xml_does_not_publish_first_shard(self):
        self.shards[self.second] = bz2.compress(b"<?xml version='1.0'?><mediawiki><page>")
        self.write_inventory()
        with self.assertRaisesRegex(DumpExtractionError, "malformed"):
            self.extract()
        self.assert_no_publication()

    def test_rejects_overlap_wrong_project_and_unsafe_inventory(self):
        self.inventory.write_text(
            f"{'a' * 64}  {self.first}\n"
            f"{'a' * 64}  enwiki-2026-10-01-p100p300.xml.bz2\n"
        )
        with self.assertRaisesRegex(DumpExtractionError, "overlapping"):
            self.extract()
        self.assert_no_publication()
        self.write_inventory()
        with self.assertRaisesRegex(DumpExtractionError, "identity"):
            extract_current_export(self.stage, self.inventory, "frwiki",
                                   "2026-10-01", self.output)
        self.assert_no_publication()
        self.inventory.write_text(f"{'a' * 64}  ../bad.xml.bz2\n")
        with self.assertRaisesRegex(DumpExtractionError, "unsafe"):
            self.extract()
        self.assert_no_publication()

    @unittest.skipUnless(hasattr(Path, "symlink_to"), "symlinks unavailable")
    def test_symlinked_member_or_staging_root_is_rejected(self):
        self.stage.joinpath(self.first).unlink()
        self.stage.joinpath(self.first).symlink_to(self.root / "missing")
        with self.assertRaisesRegex(DumpExtractionError, "unsafe"):
            self.extract()
        self.assert_no_publication()


if __name__ == "__main__":
    unittest.main()
