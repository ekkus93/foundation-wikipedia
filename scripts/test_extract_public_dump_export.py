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

    def test_aggregate_page_and_output_budgets_fail_without_partial_publication(self):
        with self.assertRaisesRegex(DumpExtractionError, "page budget"):
            self.extract(max_total_pages=1)
        self.assert_no_publication()
        with self.assertRaisesRegex(DumpExtractionError, "JSONL byte budget"):
            self.extract(max_output_bytes=20)
        self.assert_no_publication()
        with self.assertRaisesRegex(DumpExtractionError, "invalid export budget"):
            self.extract(max_total_pages=0)
        self.assert_no_publication()

    def test_changed_first_shard_after_extraction_is_rejected(self):
        from unittest.mock import patch
        import extract_public_dump_export as module
        real = module.extract_current_dump

        def mutate_after_second(*args, **kwargs):
            count = real(*args, **kwargs)
            if args[0].name == self.second:
                (self.stage / self.first).write_bytes(b"mutated after parsing")
            return count

        with patch.object(module, "extract_current_dump", side_effect=mutate_after_second):
            with self.assertRaisesRegex(DumpExtractionError, "SHA-256 mismatch"):
                self.extract()
        self.assert_no_publication()

    def test_rejects_symlinked_checksum_inventory(self):
        original = self.root / "inventory-original"
        self.inventory.rename(original)
        self.inventory.symlink_to(original)
        with self.assertRaisesRegex(DumpExtractionError, "symlinked checksum inventory"):
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


    def test_official_pages_meta_current_numbered_shards(self):
        first = "enwiki-20261001-pages-meta-current1.xml-p1p100.bz2"
        second = "enwiki-20261001-pages-meta-current2.xml-p101p200.bz2"
        self.shards = {first: bz2.compress(xml(10)),
                       second: bz2.compress(xml(110))}
        self.write_inventory()
        self.assertEqual(self.extract(), 2)
        entries = [json.loads(line) for line in self.output.read_text().splitlines()]
        self.assertEqual([entry["page_id"] for entry in entries], [10, 110])
        self.assertEqual([entry["source_text"] for entry in entries],
                         ["Wiki 10", "Wiki 110"])

    def test_rejects_mixed_export_formats_and_wrong_legacy_members(self):
        legacy = "enwiki-20261001-pages-meta-current2.xml-p101p200.bz2"
        self.shards = {self.first: bz2.compress(xml(10)),
                       legacy: bz2.compress(xml(110))}
        self.write_inventory()
        with self.assertRaisesRegex(DumpExtractionError, "mixed current-content"):
            self.extract()
        self.assert_no_publication()

        for invalid in (
            "enwiki-20260901-pages-meta-current2.xml-p101p200.bz2",
            "enwiki-20261001-pages-meta-history2.xml-p101p200.bz2",
            "enwiki-20261001-pages-meta-current2.xml-p101r10r20.bz2",
            "enwiki-20261001-pages-meta-current0.xml-p101p200.bz2",
        ):
            with self.subTest(member=invalid):
                self.shards = {
                    "enwiki-20261001-pages-meta-current1.xml-p1p100.bz2":
                        bz2.compress(xml(10)),
                    invalid: bz2.compress(xml(110)),
                }
                self.write_inventory()
                with self.assertRaisesRegex(DumpExtractionError, "member identity"):
                    self.extract()
                self.assert_no_publication()


    def test_undeclared_staging_entries_fail_closed_before_publishing(self):
        extra_file = self.stage / "not-in-SHA256SUMS.xml.bz2"
        extra_file.write_bytes(b"untrusted")
        with self.assertRaisesRegex(DumpExtractionError, "undeclared staged export file"):
            self.extract()
        self.assert_no_publication()
        extra_file.unlink()

        extra_directory = self.stage / "leftover"
        extra_directory.mkdir()
        with self.assertRaisesRegex(DumpExtractionError, "undeclared staged export directory"):
            self.extract()
        self.assert_no_publication()
        extra_directory.rmdir()

        temp_file = self.stage / ".incomplete.tmp"
        temp_file.write_bytes(b"partial")
        with self.assertRaisesRegex(DumpExtractionError, "undeclared staged export file"):
            self.extract()
        self.assert_no_publication()
        temp_file.unlink()

    def test_exact_tree_rejects_symlinks_even_when_extra_alias_is_checksum_valid(self):
        alias = self.stage / "additional-alias"
        try:
            alias.symlink_to(self.stage / self.first)
        except (OSError, NotImplementedError):
            self.skipTest("symlink creation unsupported")
        with self.assertRaisesRegex(DumpExtractionError, "unsafe staged export entry"):
            self.extract()
        self.assert_no_publication()
        alias.unlink()

    def test_exact_tree_allows_only_declared_nested_paths(self):
        first = "nested/enwiki-2026-10-01-p1p100.xml.bz2"
        second = "enwiki-2026-10-01-p101p200.xml.bz2"
        (self.stage / self.first).unlink()
        nested = self.stage / "nested"
        nested.mkdir()
        self.shards = {
            first: bz2.compress(xml(10)),
            second: bz2.compress(xml(110)),
        }
        (self.stage / first).write_bytes(self.shards[first])
        self.inventory.write_text("".join(
            f"{hashlib.sha256(data).hexdigest()}  {name}\n"
            for name, data in self.shards.items()
        ))
        self.assertEqual(self.extract(), 2)
        self.assertEqual(len(self.output.read_text().splitlines()), 2)

    def test_rejects_overlapping_public_numbered_shards(self):
        self.shards = {
            "enwiki-20261001-pages-meta-current1.xml-p1p100.bz2":
                bz2.compress(xml(10)),
            "enwiki-20261001-pages-meta-current2.xml-p100p200.bz2":
                bz2.compress(xml(110)),
        }
        self.write_inventory()
        with self.assertRaisesRegex(DumpExtractionError, "overlapping"):
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
