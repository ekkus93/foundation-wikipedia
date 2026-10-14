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

    def test_main_slot_ancestry_and_multilingual_markup_are_preserved(self):
        # MediaWiki export-0.11 provides optional parentid and origin for
        # revision ancestry/main-slot provenance; raw wikitext stays unrendered.
        message = "عِلْم 中文 \u200fπ = 3.14; {{Infobox}} <ref name='x'>مرجع</ref>"
        escaped = (message.replace("&", "&amp;").replace("<", "&lt;")
                   .replace(">", "&gt;")).encode("utf-8")
        page = XML.replace(
            b"<revision><id>50</id>",
            b"<revision><id>50</id><parentid>40</parentid><origin>45</origin>",
            1,
        ).replace(b"Energy = mc^2", escaped, 1)
        self.write_xml(page)
        self.assertEqual(self.extract(), 2)
        record = json.loads(self.output.read_text(encoding="utf-8").splitlines()[0])
        self.assertEqual(record["parent_revision_id"], 40)
        self.assertEqual(record["main_origin_revision_id"], 45)
        self.assertEqual(record["revision_id"], 50)
        self.assertEqual(record["wikitext"], message)
        self.assertEqual(record["source_text"], message)
        self.assertIsNone(record["redirect_target"])
        self.assertIsNone(json.loads(self.output.read_text().splitlines()[1])
                          ["parent_revision_id"])

    def test_invalid_parent_or_main_slot_origin_fail_before_output(self):
        for element in ("parentid", "origin"):
            for invalid in ("0", "-1", "01", "bad", "9223372036854775808"):
                with self.subTest(element=element, invalid=invalid):
                    replacement = (
                        b"<revision><id>50</id><" + element.encode() + b">"
                        + invalid.encode() + b"</" + element.encode() + b">"
                    )
                    self.write_xml(
                        XML.replace(b"<revision><id>50</id>", replacement, 1)
                    )
                    with self.assertRaisesRegex(DumpExtractionError, "identity"):
                        self.extract()
                    self.assertFalse(self.output.exists())
                    self.assertFalse(list(self.root.glob(".dump-pages.*.tmp")))

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

    def test_extra_mcr_content_slots_preserve_model_role_origin_and_bytes(self):
        additional = (
            b"<content><role>mediainfo</role><origin>50</origin>"
            b"<model>wikibase-mediainfo</model><format>application/json</format>"
            b'<text bytes="2">{}</text></content>'
        )
        self.write_xml(XML.replace(b"</text></revision>",
                                   b"</text>" + additional + b"</revision>", 1))
        self.assertEqual(self.extract(), 2)
        first = json.loads(self.output.read_text().splitlines()[0])
        self.assertEqual(first["content_model"], "wikitext")
        self.assertEqual(first["content_format"], "text/x-wiki")
        self.assertEqual(first["source_text"], first["wikitext"])
        self.assertEqual(first["additional_slots"], [{
            "role": "mediainfo", "origin_revision_id": 50,
            "content_model": "wikibase-mediainfo",
            "content_format": "application/json", "source_text": "{}",
        }])

    def test_non_wikitext_main_content_is_not_mislabeled_as_wikitext(self):
        extra = b"<model>Scribunto</model><format>text/plain</format>"
        self.write_xml(XML.replace(b"<text xml:space", extra + b"<text xml:space", 1))
        self.assertEqual(self.extract(), 2)
        first = json.loads(self.output.read_text().splitlines()[0])
        self.assertEqual(first["content_model"], "Scribunto")
        self.assertEqual(first["source_text"], "Energy = mc^2")
        self.assertIsNone(first["wikitext"])

    def test_missing_or_duplicate_mcr_roles_reject_entire_shard(self):
        cases = (
            b"<content><role>main</role><origin>50</origin>"
            b"<model>wikitext</model><format>text/x-wiki</format>"
            b"<text>duplicate</text></content>",
            b"<content><role>mediainfo</role><model>json</model>"
            b"<format>application/json</format><text>{}</text></content>",
            b"<content><role>mediainfo</role><origin>50</origin>"
            b"<model>json</model><format>application/json</format>"
            b'<text bytes="10">{}</text></content>',
        )
        for content in cases:
            with self.subTest(content=content):
                self.write_xml(XML.replace(b"</text></revision>",
                                           b"</text>" + content + b"</revision>", 1))
                with self.assertRaises(DumpExtractionError):
                    self.extract()
                self.assertFalse(self.output.exists())
                self.assertFalse(list(self.root.glob(".dump-pages.*.tmp")))

    def test_escaped_wikitext_markup_roundtrips_without_xml_children(self):
        body = b"&lt;ref&gt;A &amp; B&lt;/ref&gt;"
        self.write_xml(XML.replace(b"Energy = mc^2", body))
        self.assertEqual(self.extract(), 2)
        first = json.loads(self.output.read_text().splitlines()[0])
        self.assertEqual(first["wikitext"], "<ref>A & B</ref>")

    def test_unsupported_xml_nesting_and_namespace_fail_before_publication(self):
        cases = (
            (XML.replace(b"<page>", b"<siteinfo><page>", 1)
                .replace(b"</page>", b"</page></siteinfo>", 1),
             "page outside"),
            (XML.replace(b"http://www.mediawiki.org/xml/export-0.11/",
                         b"http://untrusted.example/export-0.11/"), "namespace"),
            (XML.replace(b"<page>", b"<wrap>" * 65 + b"<page>", 1)
                .replace(b"</page>", b"</page>" + b"</wrap>" * 65, 1), "nesting"),
        )
        for malformed, error in cases:
            with self.subTest(error=error):
                self.write_xml(malformed)
                with self.assertRaisesRegex(DumpExtractionError, error):
                    self.extract()
                self.assertFalse(self.output.exists())
                self.assertFalse(list(self.root.glob(".dump-pages.*.tmp")))

    def test_foreign_namespace_page_and_identity_fields_fail_closed(self):
        # ElementTree's {*}name wildcard matches foreign namespaces, so
        # reject spoofed page/identity elements before record extraction.
        cases = (
            XML.replace(b"<page>", b'<x:page xmlns:x="urn:spoof">', 1)
               .replace(b"</page>", b"</x:page>", 1),
            XML.replace(b"<id>10</id>", b'<x:id xmlns:x="urn:spoof">10</x:id>', 1),
            XML.replace(b"<title>Physics</title>",
                        b'<x:title xmlns:x="urn:spoof">Physics</x:title>', 1),
            XML.replace(b"<text xml:space=",
                        b'<x:text xmlns:x="urn:spoof" xml:space=', 1)
               .replace(b"</text>", b"</x:text>", 1),
        )
        for spoofed in cases:
            with self.subTest(spoofed=spoofed[:90]):
                self.write_xml(spoofed)
                with self.assertRaisesRegex(DumpExtractionError, "foreign XML element namespace"):
                    self.extract()
                self.assertFalse(self.output.exists())
                self.assertFalse(list(self.root.glob(".dump-pages.*.tmp")))

    def test_valid_siteinfo_sibling_remains_supported(self):
        siteinfo = b"<siteinfo><sitename>Wikipedia</sitename></siteinfo>"
        self.write_xml(XML.replace(b"<page>", siteinfo + b"<page>", 1))
        self.assertEqual(self.extract(), 2)

    def test_siteinfo_database_identity_matches_claimed_dump_project(self):
        valid = b"<siteinfo><sitename>Wikipedia</sitename><dbname>enwiki</dbname></siteinfo>"
        self.write_xml(XML.replace(b"<page>", valid + b"<page>", 1))
        self.assertEqual(self.extract(), 2)
        self.output.unlink()
        for dbname in (b"frwiki", b"Enwiki", b"enwiki2", b"", b"enwiki "):
            with self.subTest(dbname=dbname):
                forged = valid.replace(b"<dbname>enwiki</dbname>",
                                       b"<dbname>" + dbname + b"</dbname>")
                self.write_xml(XML.replace(b"<page>", forged + b"<page>", 1))
                with self.assertRaisesRegex(DumpExtractionError, "siteinfo project mismatch"):
                    self.extract()
                self.assertFalse(self.output.exists())
                self.assertFalse(list(self.root.glob(".dump-pages.*.tmp")))

    def test_duplicate_late_or_nested_siteinfo_rejected_without_publication(self):
        siteinfo = b"<siteinfo><dbname>enwiki</dbname></siteinfo>"
        cases = (
            XML.replace(b"<page>", siteinfo + siteinfo + b"<page>", 1),
            XML.replace(b"</mediawiki>", siteinfo + b"</mediawiki>"),
            XML.replace(b"<page>", b"<page>" + siteinfo, 1),
            XML.replace(b"<page>", b"<siteinfo><dbname><em>enwiki</em></dbname>"
                        b"</siteinfo><page>", 1),
        )
        for candidate in cases:
            with self.subTest(candidate=candidate[:110]):
                self.write_xml(candidate)
                with self.assertRaisesRegex(
                    DumpExtractionError, "siteinfo|nested XML",
                ):
                    self.extract()
                self.assertFalse(self.output.exists())
                self.assertFalse(list(self.root.glob(".dump-pages.*.tmp")))

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
            (b'<text xml:space="preserve">Energy = mc^2</text>',
             b'<text xml:space="preserve">Energy = mc^2</text><text>lost</text>'),
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

    def test_rejects_noncanonical_ids_namespaces_and_revision_times(self):
        cases = (
            (b"<id>10</id>", b"<id>+10</id>", "identity"),
            (b"<id>10</id>", b"<id>01</id>", "identity"),
            (b"<id>50</id>", b"<id>5_0</id>", "identity"),
            (b"<id>10</id>", b"<id>9223372036854775808</id>", "identity"),
            (b"<ns>0</ns>", b"<ns>1_0</ns>", "namespace"),
            (b"<ns>0</ns>", b"<ns>-0</ns>", "namespace"),
            (b"<ns>0</ns>", b"<ns>2147483648</ns>", "namespace"),
            (b"<timestamp>2026-10-01T00:00:00Z</timestamp>",
             b"<timestamp>2026-02-30T00:00:00Z</timestamp>", "timestamp"),
            (b"<timestamp>2026-10-01T00:00:00Z</timestamp>",
             b"<timestamp>2026-10-01T00:00:00+00:00</timestamp>", "timestamp"),
        )
        for old, new, diagnostic in cases:
            with self.subTest(new=new):
                self.write_xml(XML.replace(old, new, 1))
                with self.assertRaisesRegex(DumpExtractionError, diagnostic):
                    self.extract()
                self.assertFalse(self.output.exists())
                self.assertFalse(list(self.root.glob(".dump-pages.*.tmp")))

    def test_real_wikimedia_meta_current_partition_filename(self):
        published = self.root / "enwiki-20261001-pages-meta-current1.xml-p1p100.bz2"
        self.shard.rename(published)
        self.shard = published
        self.assertEqual(self.extract(), 2)
        self.assertEqual(
            [json.loads(line)["page_id"] for line in self.output.read_text().splitlines()],
            [10, 20],
        )

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

    def test_rejects_out_of_order_unique_page_ids_before_publication(self):
        start = XML.index(b"  <page>")
        second = XML.index(b"  <page>", start + 1)
        end = XML.index(b"</mediawiki>")
        # Both pages and revisions are individually valid and distinct.
        # Reversing their XML order must fail before publishing JSONL.
        self.write_xml(XML[:start] + XML[second:end] + XML[start:second] + XML[end:])
        with self.assertRaisesRegex(DumpExtractionError, "out-of-order"):
            self.extract()
        self.assertFalse(self.output.exists())
        self.assertFalse(list(self.root.glob(".dump-pages.*.tmp")))

    def test_rejects_reused_revision_id_across_distinct_page_ids(self):
        # A single Wikimedia revision belongs to exactly one page. An
        # otherwise valid shard must not publish conflicting provenance.
        self.write_xml(XML.replace(
            b"<revision><id>60</id>", b"<revision><id>50</id>", 1,
        ))
        with self.assertRaisesRegex(DumpExtractionError, "duplicate revision ID"):
            self.extract()
        self.assertFalse(self.output.exists())
        self.assertFalse(list(self.root.glob(".dump-pages.*.tmp")))

    def test_bounded_decompression_and_page_text(self):
        with self.assertRaisesRegex(DumpExtractionError, "decompressed XML exceeds budget"):
            self.extract(max_uncompressed_bytes=64)
        self.assertFalse(self.output.exists())
        with self.assertRaisesRegex(DumpExtractionError, "oversized page"):
            self.extract(max_text_bytes=5)
        self.assertFalse(self.output.exists())

    def test_compressed_input_budget_is_checked_before_hash_or_publication(self):
        with self.assertRaisesRegex(DumpExtractionError, "compressed source exceeds budget"):
            self.extract(max_compressed_bytes=8)
        self.assertFalse(self.output.exists())
        self.assertFalse(list(self.root.glob(".dump-pages.*.tmp")))
        with self.assertRaisesRegex(DumpExtractionError, "invalid extraction budget"):
            self.extract(max_compressed_bytes=0)
        self.assertEqual(self.extract(max_compressed_bytes=self.shard.stat().st_size), 2)

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
