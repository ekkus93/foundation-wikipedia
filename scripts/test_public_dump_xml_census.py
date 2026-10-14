"""Deterministic end-to-end bzip2/XML census decoding regressions."""
import bz2
import hashlib
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from public_dump_xml_census import DumpDecodeError, stage_observed_census


def page(pid, ns=0, revision=300):
    return (f"<page><title>Page {pid}</title><ns>{ns}</ns><id>{pid}</id>"
            f"<revision><id>{revision}</id><timestamp>2026-10-01T00:00:00Z</timestamp>"
            f"<contributor><id>999999</id></contributor><text>Sample</text></revision></page>")


class DumpXmlTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.staging = self.root / "staging"
        self.output = self.root / "private"
        self.staging.mkdir()
        self.output.mkdir(mode=0o700)
        self.manifest = {
            "completed": True,
            "project": "enwiki",
            "generation_id": "2026-10-01",
            "source_url": "https://dumps.wikimedia.org/other/mediawiki_content_current/enwiki/2026-10-01/xml/bzip2/SHA256SUMS",
            "files": [],
        }

    def add(self, first, last, xml):
        name = f"enwiki-2026-10-01-p{first}p{last}.xml.bz2"
        data = bz2.compress(xml.encode("utf-8"))
        (self.staging / name).write_bytes(data)
        self.manifest["files"].append({"name": name, "sha256": hashlib.sha256(data).hexdigest(), "bytes": len(data)})
        return self.staging / name

    def run_census(self):
        return stage_observed_census(self.manifest, self.staging, self.output, "observed.ids")

    def test_verified_bzip2_xml_members_publish_selected_ids(self):
        self.add(1, 100, f"<mediawiki>{page(1)}{page(2, ns=1)}</mediawiki>")
        self.add(201, 300, f"<mediawiki>{page(201)}</mediawiki>")
        path, count = self.run_census()
        self.assertEqual(count, 2)
        self.assertEqual(path.read_bytes(), b"1\n201\n")
        with self.assertRaises(FileExistsError):
            self.run_census()
        self.assertEqual(path.read_bytes(), b"1\n201\n")
        self.assertEqual(sorted(p.name for p in self.output.iterdir()), ["observed.ids"])

    def test_official_xml_namespace_accepted_and_foreign_namespace_rejected(self):
        official = "http://www.mediawiki.org/xml/export-0.11/"
        self.add(1, 100, f'<mediawiki xmlns="{official}">{page(1)}</mediawiki>')
        path, count = self.run_census()
        self.assertEqual((count, path.read_bytes()), (1, b"1\n"))
        path.unlink()
        self.manifest["files"].clear()
        self.add(1, 100, f'<mediawiki xmlns="urn:untrusted">{page(1)}</mediawiki>')
        with self.assertRaisesRegex(DumpDecodeError, "official mediawiki"):
            self.run_census()
        self.manifest["files"].clear()
        self.add(1, 100, f'<mediawiki xmlns="{official}" xmlns:bad="urn:untrusted">'
                             f'<bad:page><bad:title>Page 1</bad:title><bad:ns>0</bad:ns>'
                             f'<bad:id>1</bad:id></bad:page></mediawiki>')
        with self.assertRaisesRegex(DumpDecodeError, "root namespace"):
            self.run_census()
        self.assertEqual(list(self.output.iterdir()), [])

    def test_corrupt_compressed_member_rejected_before_decode(self):
        path = self.add(1, 100, f"<mediawiki>{page(1)}</mediawiki>")
        path.write_bytes(path.read_bytes() + b"tamper")
        with self.assertRaisesRegex(DumpDecodeError, "staging verification"):
            self.run_census()
        self.assertEqual(list(self.output.iterdir()), [])

    def test_malformed_xml_duplicate_ids_and_truncated_bzip2(self):
        for xml in [
            f"<mediawiki>{page(1)}",
            f"<mediawiki>{page(1)}{page(1)}</mediawiki>",
            f"<mediawiki>{page(101)}</mediawiki>",
            "<mediawiki><page><id>1</id></page></mediawiki>",
            f"<!DOCTYPE mediawiki [<!ENTITY x 'bad'>]><mediawiki>{page(1)}</mediawiki>",
            f"<mediawiki>{page(1)}<wrapper>{page(2)}</wrapper></mediawiki>",
        ]:
            self.manifest["files"].clear()
            self.add(1, 100, xml)
            with self.assertRaises(DumpDecodeError):
                self.run_census()
            self.assertEqual(list(self.output.iterdir()), [])

    def test_member_replaced_after_preverification_is_rejected(self):
        path = self.add(1, 100, f"<mediawiki>{page(1)}</mediawiki>")
        def replace(_manifest, _staging):
            path.write_bytes(bz2.compress(f"<mediawiki>{page(2)}</mediawiki>".encode()))
        with patch("public_dump_xml_census.verify_source_bytes", side_effect=replace):
            with self.assertRaises(DumpDecodeError):
                self.run_census()
        self.assertEqual(list(self.output.iterdir()), [])

    def test_truncated_bzip2_with_matching_digest_is_rejected(self):
        path = self.add(1, 100, f"<mediawiki>{page(1)}</mediawiki>")
        data = path.read_bytes()[:-8]
        path.write_bytes(data)
        self.manifest["files"][0]["bytes"] = len(data)
        self.manifest["files"][0]["sha256"] = hashlib.sha256(data).hexdigest()
        with self.assertRaises(DumpDecodeError):
            self.run_census()
        self.assertEqual(list(self.output.iterdir()), [])

    def test_oversized_single_page_fails_before_tree_can_grow_unbounded(self):
        self.add(1, 100, f"<mediawiki>{page(1).replace('Sample', 'x' * 4096)}</mediawiki>")
        with patch("public_dump_xml_census.MAX_PAGE_XML_BYTES", 512):
            with self.assertRaisesRegex(DumpDecodeError, "per-page memory limit"):
                self.run_census()
        self.assertEqual(list(self.output.iterdir()), [])

    def test_revision_and_contributor_ids_never_become_page_ids(self):
        self.add(1, 100, f"<mediawiki>{page(42, revision=9000)}</mediawiki>")
        path, count = self.run_census()
        self.assertEqual((count, path.read_bytes()), (1, b"42\n"))

    def test_namespace_absence_and_overlapping_ranges_fail_closed(self):
        self.add(1, 100, f"<mediawiki>{page(1, ns=1)}</mediawiki>")
        with self.assertRaisesRegex(DumpDecodeError, "no pages"):
            self.run_census()
        self.add(50, 150, f"<mediawiki>{page(50)}</mediawiki>")
        with self.assertRaisesRegex(DumpDecodeError, "overlapping"):
            self.run_census()
        self.assertEqual(list(self.output.iterdir()), [])


if __name__ == "__main__":
    unittest.main()
