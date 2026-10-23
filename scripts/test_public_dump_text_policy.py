"""Cross-language parity checks for public-dump diagnostic labels and XML."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
import xml.etree.ElementTree as ET

from audit_public_dump_revision_jsonl import RevisionAuditError, audit_revision_jsonl
from public_dump_text_policy import valid_dump_label, valid_xml_text
from public_dump_xml_census import DumpDecodeError, _page_identity


def xml_page(title="München", redirect=None):
    page = ET.Element("page")
    ET.SubElement(page, "title").text = title
    ET.SubElement(page, "ns").text = "0"
    ET.SubElement(page, "id").text = "42"
    if redirect is not None:
        ET.SubElement(page, "redirect", title=redirect)
    revision = ET.SubElement(page, "revision")
    ET.SubElement(revision, "id").text = "300"
    ET.SubElement(revision, "timestamp").text = "2026-10-01T00:00:00Z"
    return page


def json_record(title="München", redirect=None, text="Sample"):
    return {
        "schema_version": 1, "project": "enwiki",
        "generation_id": "2026-10-01", "page_id": 42, "namespace": 0,
        "title": title, "revision_id": 300,
        "timestamp": "2026-10-01T00:00:00Z", "redirect_target": redirect,
        "wikitext": text,
        "wikitext_sha256": hashlib.sha256(text.encode("utf-8")).hexdigest(),
    }


class DumpTextPolicyTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name) / "diagnostic.jsonl"

    def audit(self, record):
        self.path.write_bytes((json.dumps(record, ensure_ascii=True) + "\n").encode("utf-8"))
        return audit_revision_jsonl(self.path, "enwiki", "2026-10-01", 0, True)

    def test_invisible_bidi_control_and_noncharacter_labels_rejected_by_both(self):
        rejected = (
            "\u0001", "\u007f", "\u0085", "\u061c", "\u200b",
            "\u200e", "\u200f", "\u202a", "\u202e", "\u2060",
            "\u2066", "\u2069", "\ufeff", "\ufdd0", "\ufdef",
            "\ufffe", "\uffff", "\U0001fffe", "\U0010ffff",
        )
        for marker in rejected:
            for field in ("title", "redirect_target"):
                with self.subTest(marker=hex(ord(marker)), field=field):
                    bad = "A" + marker + "B"
                    self.assertFalse(valid_dump_label(bad))
                    with self.assertRaisesRegex(DumpDecodeError, "title"):
                        _page_identity(xml_page(
                            title=bad if field == "title" else "München",
                            redirect=bad if field == "redirect_target" else None))
                    record = json_record()
                    record[field] = bad
                    with self.assertRaisesRegex(RevisionAuditError, "title|redirect"):
                        self.audit(record)

    def test_persian_indic_and_rtl_labels_roundtrip_in_both_diagnostics(self):
        for accepted in ("می‌روم", "क्‍ष", "العربية", "עברית", "日本語", "München"):
            with self.subTest(label=accepted):
                self.assertTrue(valid_dump_label(accepted))
                page = _page_identity(xml_page(accepted, accepted))
                self.assertEqual((page[2], page[5]), (accepted, accepted))
                self.assertEqual(self.audit(json_record(accepted, accepted))[0], 1)

    def test_wikitext_xml_characters_match_rust_diagnostic_reader(self):
        for accepted in ("first\nsecond", "tab\tvalue", "line\rbreak", "می‌روم"):
            with self.subTest(accepted=accepted):
                self.assertTrue(valid_xml_text(accepted))
                self.assertEqual(self.audit(json_record(text=accepted))[0], 1)
        for rejected in ("NUL\u0000", "control\u0001", "forbidden\ufffe",
                         "forbidden\uffff", "surrogate\ud800"):
            with self.subTest(rejected=repr(rejected)):
                self.assertFalse(valid_xml_text(rejected))
                if "\ud800" not in rejected:
                    with self.assertRaisesRegex(RevisionAuditError, "XML-forbidden"):
                        self.audit(json_record(text=rejected))

    def test_scope_rejects_project_aliases_forbidden_by_rust(self):
        self.audit(json_record())
        for project in ("123wiki", "_enwiki", "enwiki_", "Enwiki"):
            with self.subTest(project=project):
                with self.assertRaisesRegex(RevisionAuditError, "scope"):
                    audit_revision_jsonl(self.path, project, "2026-10-01", 0, True)


if __name__ == "__main__":
    unittest.main()
