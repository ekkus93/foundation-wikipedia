"""Synthetic all-or-nothing current-export extraction acceptance."""
import bz2
import hashlib
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from extract_public_dump_export import (DumpExtractionError, _inventory,
                                        extract_current_export)


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
        # Each regenerated fixture describes a new generation; remove stale
        # shards from the prior version so exact-tree verification can run.
        for previous in self.stage.iterdir():
            if previous.is_file() and previous.name not in self.shards:
                previous.unlink()
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

    def test_checksum_inventory_access_time_update_is_not_a_mutation(self):
        # Some CI filesystems update atime on the first read. A read must not
        # invalidate an otherwise unchanged checksum-pinned inventory.
        original_fstat = os.fstat
        calls = 0

        def access_time_update(fd):
            nonlocal calls
            metadata = original_fstat(fd)
            calls += 1
            if calls == 2:
                class Accessed:
                    pass

                updated = Accessed()
                for field in ("st_mode", "st_nlink", "st_size", "st_dev",
                              "st_ino", "st_mtime_ns", "st_ctime_ns"):
                    setattr(updated, field, getattr(metadata, field))
                updated.st_atime = metadata.st_atime + 1
                return updated
            return metadata

        with patch("extract_public_dump_export.os.fstat",
                   side_effect=access_time_update):
            records = _inventory(self.inventory, "enwiki", "2026-10-01")
        self.assertEqual(len(records), 2)
        self.assertEqual(calls, 2)

    def test_two_shards_publish_one_ordered_revision_scoped_file(self):
        self.assertEqual(self.extract(), 2)
        entries = [json.loads(line) for line in self.output.read_text().splitlines()]
        self.assertEqual([entry["page_id"] for entry in entries], [10, 110])
        self.assertEqual([entry["wikitext"] for entry in entries], ["Wiki 10", "Wiki 110"])
        self.assertNotEqual(entries[0]["source_sha256"], entries[1]["source_sha256"])
        self.assertFalse(list(self.root.glob(".dump-export.*")))
        with self.assertRaisesRegex(DumpExtractionError, "existing output"):
            self.extract()

    def test_revalidates_decoded_jsonl_identity_and_page_partition(self):
        from extract_public_dump_pages import extract_current_dump as real_extract

        for field, value, diagnostic in (
            ("project", "frwiki", "source identity"),
            ("source_sha256", "0" * 64, "source identity"),
            ("page_id", 110, "strictly increasing"),
            ("page_id", True, "strictly increasing"),
            ("revision_id", True, "invalid decoded revision"),
            ("revision_id", 2**63, "invalid decoded revision"),
        ):
            with self.subTest(field=field, value=value):
                def forged_decoder(path, digest, project, output, **kwargs):
                    count = real_extract(path, digest, project, output, **kwargs)
                    if path.name == self.first:
                        rows = [json.loads(line) for line in output.read_text().splitlines()]
                        rows[0][field] = value
                        output.write_text(
                            "".join(json.dumps(row) + "\n" for row in rows)
                        )
                    return count

                with patch("extract_public_dump_export.extract_current_dump",
                           side_effect=forged_decoder):
                    with self.assertRaisesRegex(DumpExtractionError, diagnostic):
                        self.extract()
                self.assert_no_publication()

    def test_rejects_decoder_count_and_bounded_jsonl_violations(self):
        from extract_public_dump_pages import extract_current_dump as real_extract
        import extract_public_dump_export as export_module

        def wrong_count(path, digest, project, output, **kwargs):
            return real_extract(path, digest, project, output, **kwargs) + 1

        with patch("extract_public_dump_export.extract_current_dump",
                   side_effect=wrong_count):
            with self.assertRaisesRegex(DumpExtractionError, "count mismatch"):
                self.extract()
        self.assert_no_publication()

        def missing_newline(path, digest, project, output, **kwargs):
            count = real_extract(path, digest, project, output, **kwargs)
            if path.name == self.first:
                output.write_bytes(output.read_bytes().rstrip(b"\n"))
            return count

        with patch("extract_public_dump_export.extract_current_dump",
                   side_effect=missing_newline):
            with self.assertRaisesRegex(DumpExtractionError, "incomplete decoded JSONL"):
                self.extract()
        self.assert_no_publication()

        with patch.object(export_module, "MAX_JSONL_RECORD_BYTES", 64):
            with self.assertRaisesRegex(DumpExtractionError, "oversized"):
                self.extract()
        self.assert_no_publication()

    def test_rejects_forged_decoded_render_identity_and_content(self):
        from extract_public_dump_pages import extract_current_dump as real_extract

        for field, value, diagnostic in (
            ("namespace", True, "decoded namespace"),
            ("namespace", 2**31, "decoded namespace"),
            ("title", " bad title ", "decoded title"),
            ("title", "bad\\ncontrol", "decoded title"),
            ("timestamp", "2026-02-30T00:00:00Z", "decoded timestamp"),
            ("content_format", "application/json", "decoded content model"),
            ("source_text", [], "decoded source text"),
            ("wikitext", "forged evidence", "decoded wikitext"),
            ("redirect_target", "", "decoded redirect"),
            ("parent_revision_id", True, "decoded revision ancestry"),
            ("additional_slots", [123], "decoded additional slot"),
        ):
            with self.subTest(field=field, value=value):
                def forged_decoder(path, digest, project, output, **kwargs):
                    count = real_extract(path, digest, project, output, **kwargs)
                    if path.name == self.first:
                        rows = [json.loads(line) for line in output.read_text().splitlines()]
                        rows[0][field] = value
                        output.write_text("".join(json.dumps(row) + "\\n" for row in rows))
                    return count

                with patch("extract_public_dump_export.extract_current_dump",
                           side_effect=forged_decoder):
                    with self.assertRaisesRegex(DumpExtractionError, diagnostic):
                        self.extract()
                self.assert_no_publication()

    def test_rejects_duplicate_revision_id_across_distinct_shards(self):
        # Page partitions do not overlap, but revision identity still must.
        collision = xml(110).replace(b"<revision><id>1110</id>",
                                     b"<revision><id>1010</id>")
        self.shards[self.second] = bz2.compress(collision)
        self.write_inventory()
        with self.assertRaisesRegex(DumpExtractionError,
                                    "duplicate revision ID across export shards"):
            self.extract()
        self.assert_no_publication()

    def test_rejects_duplicate_namespace_and_title_across_shards(self):
        # Exact namespace/title conflicts cannot both be current pages,
        # even when their page IDs and revision IDs are distinct.
        title_collision = xml(110).replace(b"Article 110", b"Article 10")
        self.shards[self.second] = bz2.compress(title_collision)
        self.write_inventory()
        with self.assertRaisesRegex(DumpExtractionError,
                                    "duplicate namespace/title across export pages"):
            self.extract()
        self.assert_no_publication()

    def test_mismatched_or_missing_shard_never_publishes_partial_export(self):
        (self.stage / self.second).write_bytes(b"wrong")
        with self.assertRaisesRegex(DumpExtractionError, "SHA-256 mismatch"):
            self.extract()
        self.assert_no_publication()
        (self.stage / self.second).unlink()
        with self.assertRaisesRegex(DumpExtractionError, "missing declared staged export member"):
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

    def test_mutated_checksum_inventory_after_shard_parsing_never_publishes(self):
        from unittest.mock import patch
        import extract_public_dump_export as module
        original = module.extract_current_dump

        def rewrite_inventory_after_last_shard(*args, **kwargs):
            count = original(*args, **kwargs)
            if args[0].name == self.second:
                contents = self.inventory.read_bytes()
                replacement = b"f" if contents[:1] != b"f" else b"e"
                self.inventory.write_bytes(replacement + contents[1:])
            return count

        with patch.object(
            module, "extract_current_dump", side_effect=rewrite_inventory_after_last_shard,
        ):
            with self.assertRaisesRegex(DumpExtractionError, "inventory changed"):
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

    def test_non_month_start_public_dump_export_dates(self):
        date = "2026-10-12"
        self.shards = {
            f"enwiki-{date}-p1p100.xml.bz2": bz2.compress(xml(10)),
            f"enwiki-{date}-p101p200.xml.bz2": bz2.compress(xml(110)),
        }
        self.write_inventory()
        self.assertEqual(extract_current_export(
            self.stage, self.inventory, "enwiki", date, self.output,
        ), 2)
        self.assertEqual(len(self.output.read_text().splitlines()), 2)

    def test_invalid_calendar_date_rejected_before_publication(self):
        date = "2026-02-30"
        self.shards = {
            f"enwiki-{date}-p1p100.xml.bz2": bz2.compress(xml(10)),
        }
        self.write_inventory()
        with self.assertRaisesRegex(DumpExtractionError, "invalid export date"):
            extract_current_export(
                self.stage, self.inventory, "enwiki", date, self.output,
            )
        self.assert_no_publication()

    def test_rejects_missing_or_duplicate_numbered_legacy_shards(self):
        first = "enwiki-20261001-pages-meta-current1.xml-p1p100.bz2"
        for second in (
            "enwiki-20261001-pages-meta-current3.xml-p101p200.bz2",
            "enwiki-20261001-pages-meta-current1.xml-p101p200.bz2",
            "enwiki-20261001-pages-meta-current2.xml-p101p200.bz2",
        ):
            with self.subTest(second=second):
                self.shards = {
                    first: bz2.compress(xml(10)),
                    second: bz2.compress(xml(110)),
                }
                self.write_inventory()
                if "current2." in second:
                    self.assertEqual(self.extract(), 2)
                    self.output.unlink()
                else:
                    with self.assertRaisesRegex(DumpExtractionError, "numbered shard"):
                        self.extract()
                    self.assert_no_publication()

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
