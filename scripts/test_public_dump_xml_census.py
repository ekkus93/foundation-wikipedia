"""Deterministic end-to-end bzip2/XML census decoding regressions."""
import bz2
import hashlib
import io
import json
from pathlib import Path
import tempfile
import sys
from contextlib import redirect_stdout
import unittest
from unittest.mock import patch

from audit_public_dump_revision_jsonl import RevisionAuditError
from public_dump_xml_census import DumpDecodeError, main, stage_observed_census
from verify_source_staging import SourceVerificationError


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

    def test_revision_jsonl_exports_pinned_identity_unicode_and_redirect(self):
        first = page(1).replace("Page 1", "München")
        second = page(2, revision=777).replace(
            "<id>2</id>", '<id>2</id><redirect title="Physics"/>', 1)
        self.add(1, 100, f"<mediawiki>{first}{second}{page(3, ns=1)}</mediawiki>")
        result, count = stage_observed_census(
            self.manifest, self.staging, self.output, "revisions.jsonl",
            output_format="revision-jsonl")
        records = [json.loads(line) for line in result.read_text(encoding="utf-8").splitlines()]
        self.assertEqual(count, 2)
        self.assertEqual(records, [
            {"schema_version": 1, "project": "enwiki", "generation_id": "2026-10-01",
             "page_id": 1, "namespace": 0, "title": "München",
             "revision_id": 300, "timestamp": "2026-10-01T00:00:00Z",
             "redirect_target": None},
            {"schema_version": 1, "project": "enwiki", "generation_id": "2026-10-01",
             "page_id": 2, "namespace": 0, "title": "Page 2",
             "revision_id": 777, "timestamp": "2026-10-01T00:00:00Z",
             "redirect_target": "Physics"},
        ])
        self.assertEqual(result.read_bytes().count(b"\n"), 2)

    def test_titles_with_control_characters_rejected_for_all_formats(self):
        for title in ("Bad\nTitle", "Bad\tTitle", "Bad\rTitle"):
            self.manifest["files"].clear()
            xml = page(1).replace("Page 1", title)
            self.add(1, 100, f"<mediawiki>{xml}</mediawiki>")
            with self.assertRaisesRegex(DumpDecodeError, "page title"):
                stage_observed_census(self.manifest, self.staging, self.output, "observed.ids")
            self.assertEqual(list(self.output.iterdir()), [])

    def test_revision_content_jsonl_retains_legacy_and_main_slot_wikitext(self):
        first = page(1).replace("Sample", "MediaWiki &amp; Unicode: München\nSecond line")
        second = page(2).replace("<text>Sample</text>",
                                  '<slots><slot role="main"><model>wikitext</model>'
                                  '<text>== Heading == &amp; detail</text></slot></slots>')
        self.add(1, 100, f"<mediawiki>{first}{second}</mediawiki>")
        path, count = stage_observed_census(
            self.manifest, self.staging, self.output, "content.jsonl",
            output_format="revision-content-jsonl")
        records = [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines()]
        self.assertEqual(count, 2)
        self.assertEqual(records[0]["wikitext"], "MediaWiki & Unicode: München\nSecond line")
        self.assertEqual(records[1]["wikitext"], "== Heading == & detail")
        for record in records:
            self.assertEqual((record["schema_version"], record["project"],
                              record["generation_id"]), (1, "enwiki", "2026-10-01"))
            self.assertEqual(record["wikitext_sha256"],
                             hashlib.sha256(record["wikitext"].encode("utf-8")).hexdigest())
            self.assertEqual(record["timestamp"], "2026-10-01T00:00:00Z")
        self.assertEqual(sorted(p.name for p in self.output.iterdir()), ["content.jsonl"])

    def test_revision_content_jsonl_rejects_non_wikitext_models_and_formats(self):
        original = page(1)
        invalid = (
            "<model>json</model><text>Sample</text>",
            "<model>wikitext</model><model>wikitext</model><text>Sample</text>",
            "<format>application/json</format><text>Sample</text>",
            '<slots><slot role="main"><model>Scribunto</model>'
            '<text>Sample</text></slot></slots>',
            '<slots><slot role="main" model="json"><text>Sample</text></slot></slots>',
            '<slots><slot role="main"><format>application/json</format>'
            '<text>Sample</text></slot></slots>',
        )
        for replacement in invalid:
            with self.subTest(replacement=replacement):
                self.manifest["files"].clear()
                self.add(1, 100, "<mediawiki>" +
                         original.replace("<text>Sample</text>", replacement) +
                         "</mediawiki>")
                with self.assertRaisesRegex(DumpDecodeError, "wikitext declaration"):
                    stage_observed_census(
                        self.manifest, self.staging, self.output, "content.jsonl",
                        output_format="revision-content-jsonl")
                self.assertEqual(list(self.output.iterdir()), [])

        self.manifest["files"].clear()
        legacy = original.replace("<text>Sample</text>",
                                  "<model>wikitext</model><format>text/x-wiki</format>"
                                  "<text>Sample</text>")
        slotted = page(2).replace(
            "<text>Sample</text>",
            '<slots><slot role="main" model="wikitext">'
            '<model>wikitext</model><format>text/x-wiki</format>'
            '<text>Sample</text></slot></slots>')
        self.add(1, 100, f"<mediawiki>{legacy}{slotted}</mediawiki>")
        result, count = stage_observed_census(
            self.manifest, self.staging, self.output, "content.jsonl",
            output_format="revision-content-jsonl")
        self.assertEqual(count, 2)
        self.assertEqual([json.loads(line)["wikitext"] for line in
                          result.read_text(encoding="utf-8").splitlines()],
                         ["Sample", "Sample"])

    def test_revision_content_jsonl_enforces_declared_utf8_text_bytes(self):
        legacy = page(1).replace("<text>Sample</text>",
                                 '<text bytes="6">Sample</text>')
        modern = page(2).replace("<text>Sample</text>",
                                 '<slots><slot role="main"><model>wikitext</model>'
                                 '<text bytes="8">München</text></slot></slots>')
        self.add(1, 100, f"<mediawiki>{legacy}{modern}</mediawiki>")
        result, count = stage_observed_census(
            self.manifest, self.staging, self.output, "content.jsonl",
            output_format="revision-content-jsonl")
        self.assertEqual(count, 2)
        self.assertEqual([json.loads(line)["wikitext"] for line in
                          result.read_text(encoding="utf-8").splitlines()],
                         ["Sample", "München"])
        result.unlink()

        invalid = (
            '<text bytes="5">Sample</text>',
            '<text bytes="0">Sample</text>',
            '<text bytes="-1">Sample</text>',
            '<text bytes="06">Sample</text>',
            '<text bytes="six">Sample</text>',
            '<text bytes="99999999999999">Sample</text>',
            '<slots><slot role="main"><text bytes="7">München</text></slot></slots>',
        )
        for replacement in invalid:
            with self.subTest(replacement=replacement):
                self.manifest["files"].clear()
                self.add(1, 100, "<mediawiki>" +
                         page(1).replace("<text>Sample</text>", replacement) +
                         "</mediawiki>")
                with self.assertRaisesRegex(DumpDecodeError, "wikitext byte length"):
                    stage_observed_census(
                        self.manifest, self.staging, self.output, "content.jsonl",
                        output_format="revision-content-jsonl")
                self.assertEqual(list(self.output.iterdir()), [])

    def test_revision_content_jsonl_rejects_missing_or_ambiguous_main_text(self):
        bad_texts = (
            "",
            "<text>one</text><text>two</text>",
            '<text deleted="deleted"/>',
            '<text><nested/></text>',
            '<slots><slot role="other"><text>wrong</text></slot></slots>',
            '<slots><slot role="main"><text>one</text></slot>'
            '<slot role="main"><text>two</text></slot></slots>',
            '<text>one</text><slots><slot role="main"><text>two</text></slot></slots>',
        )
        for replacement in bad_texts:
            self.manifest["files"].clear()
            bad = page(1).replace("<text>Sample</text>", replacement)
            self.add(1, 100, f"<mediawiki>{bad}</mediawiki>")
            with self.subTest(replacement=replacement):
                with self.assertRaisesRegex(DumpDecodeError, "revision|wikitext"):
                    stage_observed_census(
                        self.manifest, self.staging, self.output, "content.jsonl",
                        output_format="revision-content-jsonl")
                self.assertEqual(list(self.output.iterdir()), [])

    def test_independent_expected_id_gate_applies_to_both_revision_formats(self):
        self.add(1, 100, f"<mediawiki>{page(1)}{page(2)}</mediawiki>")
        expected = self.root / "expected.ids"
        for output_format in ("revision-jsonl", "revision-content-jsonl"):
            with self.subTest(output_format=output_format):
                expected.write_bytes(b"1\n2\n")
                digest = hashlib.sha256(expected.read_bytes()).hexdigest()
                result, count = stage_observed_census(
                    self.manifest, self.staging, self.output, "revisions.jsonl",
                    output_format=output_format, expected_census=expected,
                    expected_sha256=digest)
                self.assertEqual(count, 2)
                self.assertEqual([json.loads(line)["page_id"] for line in
                                  result.read_text(encoding="utf-8").splitlines()], [1, 2])
                result.unlink()
                for bad in (b"1\n3\n", b"1\n", b"1\n2\n3\n"):
                    expected.write_bytes(bad)
                    with self.assertRaises(DumpDecodeError):
                        stage_observed_census(
                            self.manifest, self.staging, self.output, "revisions.jsonl",
                            output_format=output_format, expected_census=expected,
                            expected_sha256=hashlib.sha256(bad).hexdigest())
                    self.assertEqual(list(self.output.iterdir()), [])
                expected.write_bytes(b"1\n2\n")
                with self.assertRaisesRegex(DumpDecodeError, "digest"):
                    stage_observed_census(
                        self.manifest, self.staging, self.output, "revisions.jsonl",
                        output_format=output_format, expected_census=expected,
                        expected_sha256="0" * 64)
                self.assertEqual(list(self.output.iterdir()), [])

    def test_revision_jsonl_rejects_unsafe_redirect(self):
        self.add(1, 100, f"<mediawiki>{page(1)}</mediawiki>")
        for attr in ('title=" "', 'title=" Bad "', 'title="Physics" other="a"'):
            self.manifest["files"].clear()
            bad = page(1).replace("<id>1</id>", f"<id>1</id><redirect {attr}/>", 1)
            self.add(1, 100, f"<mediawiki>{bad}</mediawiki>")
            with self.assertRaisesRegex(DumpDecodeError, "redirect"):
                stage_observed_census(self.manifest, self.staging, self.output,
                                      "revisions.jsonl", output_format="revision-jsonl")
            self.assertEqual(list(self.output.iterdir()), [])
        self.manifest["files"].clear()
        bad = page(1).replace("<id>1</id>",
                               '<id>1</id><redirect title="Physics"/><redirect title="Math"/>', 1)
        self.add(1, 100, f"<mediawiki>{bad}</mediawiki>")
        with self.assertRaisesRegex(DumpDecodeError, "redirect"):
            stage_observed_census(self.manifest, self.staging, self.output,
                                  "revisions.jsonl", output_format="revision-jsonl")
        self.assertEqual(list(self.output.iterdir()), [])

    def test_revision_jsonl_self_audit_rejection_never_publishes(self):
        self.add(1, 100, f"<mediawiki>{page(1)}</mediawiki>")
        with patch("public_dump_xml_census.audit_revision_jsonl",
                   side_effect=RevisionAuditError("synthetic audit failure")):
            with self.assertRaisesRegex(DumpDecodeError, "self-audit failed"):
                stage_observed_census(
                    self.manifest, self.staging, self.output, "metadata.jsonl",
                    output_format="revision-jsonl")
        self.assertEqual(list(self.output.iterdir()), [])
        with patch("public_dump_xml_census.audit_revision_jsonl",
                   return_value=(0, "0" * 64)):
            with self.assertRaisesRegex(DumpDecodeError, "count mismatch"):
                stage_observed_census(
                    self.manifest, self.staging, self.output, "metadata.jsonl",
                    output_format="revision-jsonl")
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

    def test_excessive_xml_nesting_rejected_without_publication(self):
        nested = "<siteinfo>" + "<nested>" * 140 + "</nested>" * 140 + "</siteinfo>"
        self.add(1, 100, f"<mediawiki>{nested}{page(1)}</mediawiki>")
        with self.assertRaisesRegex(DumpDecodeError, "depth limit"):
            self.run_census()
        self.assertEqual(list(self.output.iterdir()), [])

    def test_invalid_calendar_revision_timestamp_rejected(self):
        for invalid in ("2026-02-30", "2026-13-01", "2026-10-01T25:00:00"):
            self.manifest["files"].clear()
            xml = page(1).replace("2026-10-01T00:00:00", invalid + ("" if "T" in invalid else "T00:00:00"))
            self.add(1, 100, f"<mediawiki>{xml}</mediawiki>")
            with self.assertRaises(DumpDecodeError):
                self.run_census()
            self.assertEqual(list(self.output.iterdir()), [])

    def test_revision_and_contributor_ids_never_become_page_ids(self):
        self.add(1, 100, f"<mediawiki>{page(42, revision=9000)}</mediawiki>")
        path, count = self.run_census()
        self.assertEqual((count, path.read_bytes()), (1, b"42\n"))

    def test_digest_pinned_expected_census_gate_before_publication(self):
        self.add(1, 100, f"<mediawiki>{page(1)}{page(2)}</mediawiki>")
        expected = self.root / "independent.ids"
        expected.write_bytes(b"1\n2\n")
        digest = hashlib.sha256(expected.read_bytes()).hexdigest()
        result, count = stage_observed_census(
            self.manifest, self.staging, self.output, "observed.ids",
            expected_census=expected, expected_sha256=digest)
        self.assertEqual((count, result.read_bytes()), (2, b"1\n2\n"))
        result.unlink()
        for bad in (b"1\n3\n", b"1\n", b"1\n2\n3\n", b"2\n1\n",
                    b"01\n2\n", b"1\n2", b"1\n2\nextra\n"):
            expected.write_bytes(bad)
            with self.assertRaises(DumpDecodeError):
                stage_observed_census(
                    self.manifest, self.staging, self.output, "observed.ids",
                    expected_census=expected,
                    expected_sha256=hashlib.sha256(bad).hexdigest())
            self.assertEqual(list(self.output.iterdir()), [])
        expected.write_bytes(b"1\n2\n")
        with self.assertRaisesRegex(DumpDecodeError, "digest"):
            stage_observed_census(
                self.manifest, self.staging, self.output, "observed.ids",
                expected_census=expected, expected_sha256="0" * 64)
        self.assertEqual(list(self.output.iterdir()), [])

    def test_expected_gate_rejects_missing_digest_and_symlink(self):
        self.add(1, 100, f"<mediawiki>{page(1)}</mediawiki>")
        expected = self.root / "expected.ids"
        expected.write_bytes(b"1\n")
        with self.assertRaisesRegex(DumpDecodeError, "supplied together"):
            stage_observed_census(self.manifest, self.staging, self.output,
                                  "observed.ids", expected_census=expected)
        link = self.root / "expected-link.ids"
        link.symlink_to(expected)
        with self.assertRaises(OSError):
            stage_observed_census(
                self.manifest, self.staging, self.output, "observed.ids",
                expected_census=link,
                expected_sha256=hashlib.sha256(expected.read_bytes()).hexdigest())
        self.assertEqual(list(self.output.iterdir()), [])

    def test_siteinfo_is_accepted_but_unexpected_top_level_xml_fails(self):
        self.add(1, 100, f"<mediawiki><siteinfo><sitename>Test</sitename></siteinfo>{page(1)}</mediawiki>")
        result, count = self.run_census()
        self.assertEqual((count, result.read_bytes()), (1, b"1\n"))
        result.unlink()
        for xml in (
            f"<mediawiki><unknown/>{page(1)}</mediawiki>",
            f"<mediawiki><siteinfo/><siteinfo/>{page(1)}</mediawiki>",
            f"<mediawiki>{page(1)}<siteinfo/></mediawiki>",
        ):
            self.manifest["files"].clear()
            self.add(1, 100, xml)
            with self.assertRaises(DumpDecodeError):
                self.run_census()
            self.assertEqual(list(self.output.iterdir()), [])

    def test_oversized_siteinfo_rejected_without_publication(self):
        self.add(1, 100, f"<mediawiki><siteinfo><description>{'x' * 4096}</description></siteinfo>{page(1)}</mediawiki>")
        with patch("public_dump_xml_census.MAX_NON_PAGE_XML_BYTES", 512):
            with self.assertRaisesRegex(DumpDecodeError, "metadata memory limit"):
                self.run_census()
        self.assertEqual(list(self.output.iterdir()), [])

    def test_extremely_long_decimal_identifiers_fail_closed(self):
        valid = page(1)
        for bad in (
            valid.replace("<id>1</id>", "<id>" + "9" * 5000 + "</id>", 1),
            valid.replace("<ns>0</ns>", "<ns>" + "9" * 5000 + "</ns>"),
            valid.replace("<revision><id>300</id>",
                          "<revision><id>" + "9" * 5000 + "</id>"),
        ):
            self.manifest["files"].clear()
            self.add(1, 100, f"<mediawiki>{bad}</mediawiki>")
            with self.assertRaises(DumpDecodeError):
                self.run_census()
            self.assertEqual(list(self.output.iterdir()), [])

    def test_extremely_long_shard_id_and_namespace_argument_fail_closed(self):
        huge = "9" * 100
        self.add(huge, huge, f"<mediawiki>{page(1)}</mediawiki>")
        with self.assertRaisesRegex(DumpDecodeError, "shard range"):
            self.run_census()
        self.assertEqual(list(self.output.iterdir()), [])
        with self.assertRaisesRegex(DumpDecodeError, "namespace"):
            stage_observed_census(self.manifest, self.staging, self.output,
                                  "observed.ids", namespace=2**31)
        self.assertEqual(list(self.output.iterdir()), [])

    def test_cli_uses_bounded_manifest_loader_for_valid_input(self):
        self.add(1, 100, f"<mediawiki>{page(1)}</mediawiki>")
        manifest_file = self.root / "source.json"
        manifest_file.write_text(json.dumps(self.manifest), encoding="utf-8")
        argv = ["public_dump_xml_census.py", str(manifest_file),
                str(self.staging), str(self.output), "observed.ids"]
        with patch.object(sys, "argv", argv), redirect_stdout(io.StringIO()) as output:
            main()
        self.assertEqual((self.output / "observed.ids").read_bytes(), b"1\n")
        self.assertIn("authenticity and completeness NOT established", output.getvalue())

    def test_cli_rejects_symlink_manifest_before_output(self):
        self.add(1, 100, f"<mediawiki>{page(1)}</mediawiki>")
        original = self.root / "source.json"
        original.write_text(json.dumps(self.manifest), encoding="utf-8")
        alias = self.root / "source-alias.json"
        alias.symlink_to(original)
        argv = ["public_dump_xml_census.py", str(alias),
                str(self.staging), str(self.output), "observed.ids"]
        with patch.object(sys, "argv", argv):
            with self.assertRaises(SourceVerificationError):
                main()
        self.assertEqual(list(self.output.iterdir()), [])

    def test_cli_rejects_oversized_and_malformed_manifest_before_output(self):
        self.add(1, 100, f"<mediawiki>{page(1)}</mediawiki>")
        manifest_file = self.root / "source.json"
        argv = ["public_dump_xml_census.py", str(manifest_file),
                str(self.staging), str(self.output), "observed.ids"]
        manifest_file.write_text(json.dumps(self.manifest), encoding="utf-8")
        with patch("verify_source_staging.MAX_MANIFEST_BYTES", 16):
            with patch.object(sys, "argv", argv), self.assertRaisesRegex(
                    SourceVerificationError, "oversized"):
                main()
        manifest_file.write_text("[]", encoding="utf-8")
        with patch.object(sys, "argv", argv), self.assertRaisesRegex(
                SourceVerificationError, "JSON object"):
            main()
        self.assertEqual(list(self.output.iterdir()), [])

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
