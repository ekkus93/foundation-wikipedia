"""Offline synthetic acceptance for bounded public current-content XML extraction."""
import bz2
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from extract_public_dump_pages import DumpExtractionError, extract_current_dump


XML = b'''<?xml version="1.0" encoding="utf-8"?>
<mediawiki xmlns="http://www.mediawiki.org/xml/export-0.11/">
  <page><title>Physics</title><ns>0</ns><id>10</id>
    <revision><id>50</id><timestamp>2026-10-01T00:00:00Z</timestamp>
      <text xml:space="preserve">Energy = mc^2</text></revision></page>
  <page><title>Old title</title><ns>0</ns><id>20</id>
    <redirect title="New title"/>
    <revision><id>60</id><timestamp>2026-10-01T00:00:00Z</timestamp>
      <text>#REDIRECT [[New title]]</text></revision></page>
</mediawiki>'''


class ExtractTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.shard = self.root / "enwiki-2026-10-01-p1p100.xml.bz2"
        self.output = self.root / "pages.jsonl"
        self.write_xml(XML)

    def write_xml(self, xml):
        compressed = bz2.compress(xml)
        self.shard.write_bytes(compressed)
        self.sha = hashlib.sha256(compressed).hexdigest()

    def extract(self, **kwargs):
        return extract_current_dump(self.shard, self.sha, "enwiki", self.output, **kwargs)

    def test_extracts_revision_identity_wikitext_and_redirect(self):
        self.assertEqual(self.extract(), 2)
        records = [json.loads(line) for line in self.output.read_text().splitlines()]
        self.assertEqual([row["page_id"] for row in records], [10, 20])
        self.assertEqual(records[0]["wikitext"], "Energy = mc^2")
        self.assertEqual(records[0]["revision_id"], 50)
        self.assertEqual(records[1]["redirect_target"], "New title")
        self.assertEqual(records[1]["source_sha256"], self.sha)
        with self.assertRaisesRegex(DumpExtractionError, "already exists"):
            self.extract()

    def test_empty_current_wikitext_is_preserved(self):
        self.write_xml(XML.replace(b"Energy = mc^2", b""))
        self.assertEqual(self.extract(), 2)
        first = json.loads(self.output.read_text().splitlines()[0])
        self.assertEqual(first["wikitext"], "")

    def test_declared_wikitext_byte_count_matches_utf8(self):
        body = "Énergie π".encode("utf-8")
        xml = XML.replace(b"Energy = mc^2", body)
        xml = xml.replace(b"<text xml:space=",
                          f"<text bytes=\"{len(body)}\" xml:space=".encode(), 1)
        self.write_xml(xml)
        self.assertEqual(self.extract(), 2)
        self.assertEqual(json.loads(self.output.read_text().splitlines()[0])["wikitext"],
                         "Énergie π")
        self.output.unlink()
        for value in ("0", "-1", "01", "garbage", ""):
            malformed = xml.replace(f"bytes=\"{len(body)}\"".encode(),
                                    f"bytes=\"{value}\"".encode(), 1)
            self.write_xml(malformed)
            with self.assertRaisesRegex(DumpExtractionError, "byte count mismatch"):
                self.extract()
            self.assertFalse(self.output.exists())
            self.assertFalse(list(self.root.glob(".dump-pages.*.tmp")))

    def test_escaped_wikitext_markup_roundtrips_without_xml_children(self):
        body = b"&lt;ref&gt;A &amp; B&lt;/ref&gt;"
        self.write_xml(XML.replace(b"Energy = mc^2", body))
        self.assertEqual(self.extract(), 2)
        first = json.loads(self.output.read_text().splitlines()[0])
        self.assertEqual(first["wikitext"], "<ref>A & B</ref>")

    def test_rejects_nested_or_duplicate_scalar_xml_without_publication(self):
        # XML child elements are not wikitext: Element.text alone would silently
        # truncate them. Valid escaped wikitext is tested separately above.
        timestamp = b"2026-10-01T00:00:00Z"
        invalid = (
            (b"Energy = mc^2", b"Energy <ref>lost</ref> tail"),
            (b"<title>Physics</title>", b"<title>Phy<em>sic</em>s</title>"),
            (b"<ns>0</ns>", b"<ns><number>0</number></ns>"),
            (b"<id>10</id>", b"<id><value>10</value></id>"),
            (b"<timestamp>" + timestamp + b"</timestamp>",
             b"<timestamp>" + timestamp + b"<unexpected/></timestamp>"),
            (b"<id>50</id>", b"<id>50</id><id>51</id>"),
            (b"<title>Physics</title>",
             b"<title>Physics</title><title>Duplicate</title>"),
            (b"<text xml:space=\\"preserve\\">Energy = mc^2</text>",
             b"<text xml:space=\\"preserve\\">Energy = mc^2</text><text>lost</text>"),
            (b'<redirect title="New title"/>',
             b'<redirect title="New title"/><redirect title="Other"/>'),
        )
        for original, replacement in invalid:
            with self.subTest(replacement=replacement):
                self.write_xml(XML.replace(original, replacement, 1))
                with self.assertRaisesRegex(DumpExtractionError,
                                            "nested XML|duplicate"):
                    self.extract()
                self.assertFalse(self.output.exists())
                self.assertFalse(list(self.root.glob(".dump-pages.*.tmp")))

    def test_shard_project_and_page_range_are_bound_to_filename(self):
        with self.assertRaisesRegex(DumpExtractionError, "member identity"):
            extract_current_dump(self.shard, self.sha, "frwiki", self.output)
        narrower = self.root / "enwiki-2026-10-01-p1p10.xml.bz2"
        self.shard.rename(narrower)
        self.shard = narrower
        with self.assertRaisesRegex(DumpExtractionError, "outside declared shard"):
            self.extract()
        self.assertFalse(self.output.exists())

    def test_replaced_source_after_verification_does_not_publish(self):
        from unittest.mock import patch
        import extract_public_dump_pages as module

        original = module._verified_source

        def replace_source(*args):
            opened, metadata = original(*args)
            self.shard.rename(self.root / "original-shard")
            self.write_xml(XML.replace(b"Energy = mc^2", b"replaced"))
            return opened, metadata

        with patch.object(module, "_verified_source", side_effect=replace_source):
            with self.assertRaisesRegex(DumpExtractionError, "source changed during extraction"):
                self.extract()
        self.assertFalse(self.output.exists())
        self.assertFalse(list(self.root.glob(".dump-pages.*.tmp")))

    def test_mismatch_and_malformed_xml_leave_no_output(self):
        with self.assertRaisesRegex(DumpExtractionError, "SHA-256 mismatch"):
            extract_current_dump(self.shard, "0" * 64, "enwiki", self.output)
        self.assertFalse(self.output.exists())
        self.write_xml(b"<?xml version='1.0'?><mediawiki><page>")
        with self.assertRaises(DumpExtractionError):
            self.extract()
        self.assertFalse(self.output.exists())

    def test_rejects_duplicate_page_and_multiple_revisions(self):
        page = XML[XML.index(b"  <page>"):XML.index(b"  <page>", XML.index(b"  <page>") + 1)]
        self.write_xml(XML.replace(b"</mediawiki>", page + b"</mediawiki>"))
        with self.assertRaisesRegex(DumpExtractionError, "duplicate page"):
            self.extract()
        self.assertFalse(self.output.exists())
        self.write_xml(XML.replace(b"</revision>", b"</revision><revision><id>70</id></revision>", 1))
        with self.assertRaisesRegex(DumpExtractionError, "one revision"):
            self.extract()

    def test_bounded_decompression_and_page_text(self):
        with self.assertRaisesRegex(DumpExtractionError, "decompressed XML exceeds budget"):
            self.extract(max_uncompressed_bytes=64)
        self.assertFalse(self.output.exists())
        with self.assertRaisesRegex(DumpExtractionError, "oversized page"):
            self.extract(max_text_bytes=5)
        self.assertFalse(self.output.exists())

    def test_rejects_non_utf8_declarations_before_page_import(self):
        for encoding in (b"ISO-8859-1", b"windows-1252", b"UTF-16"):
            self.write_xml(
                b'<?xml version="1.0" encoding="' + encoding +
                b'"?><mediawiki><page/></mediawiki>'
            )
            with self.assertRaisesRegex(DumpExtractionError, "UTF-8"):
                self.extract()
            self.assertFalse(self.output.exists())
        self.write_xml(XML.replace(b'encoding="utf-8"', b'encoding="UTF-8"'))
        self.assertEqual(self.extract(), 2)

    def test_rejects_entity_declarations_and_symlinked_source(self):
        self.write_xml(b'<?xml version="1.0"?><!DOCTYPE mediawiki [<!ENTITY x "bad">]>'
                       b"<mediawiki><page/></mediawiki>")
        with self.assertRaisesRegex(DumpExtractionError, "DTD"):
            self.extract()
        self.assertFalse(self.output.exists())
        target = self.root / "target"
        self.shard.rename(target)
        self.shard.symlink_to(target)
        with self.assertRaisesRegex(DumpExtractionError, "symlinked"):
            self.extract()


if __name__ == "__main__":
    unittest.main()
