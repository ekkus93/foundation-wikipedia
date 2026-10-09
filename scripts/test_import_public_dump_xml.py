"""Synthetic streaming dump import acceptance and hostile-input regression."""
import bz2
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest

from import_public_dump_xml import DumpImportError, import_verified_member, parse_xml

BODY = b'''<mediawiki xmlns="http://www.mediawiki.org/xml/export-0.11/">
<page><title>Gravity</title><ns>0</ns><id>42</id>
<revision><id>7</id><timestamp>2026-10-09T00:00:00Z</timestamp>
<contributor><id>99</id></contributor><text xml:space="preserve">Force &amp; mass</text></revision></page>
<page><title>Redirect</title><ns>0</ns><id>43</id><redirect title="Gravity"/>
<revision><id>8</id><timestamp>2026-10-09T00:00:01Z</timestamp>
<text xml:space="preserve">#REDIRECT [[Gravity]]</text></revision></page>
</mediawiki>'''


class DumpImportTests(unittest.TestCase):
    def parse(self, body=BODY, **kwargs):
        rows = []
        parse_xml(io.BytesIO(body), rows.append, project="enwiki", generation="20261009", **kwargs)
        return rows

    def test_preserves_page_and_exact_revision_not_contributor_id(self):
        rows = self.parse()
        self.assertEqual(len(rows), 2)
        self.assertEqual(rows[0]["page_id"], 42)
        self.assertEqual(rows[0]["revision_id"], 7)
        self.assertEqual(rows[0]["wikitext"], "Force & mass")
        self.assertEqual(rows[1]["redirect_title"], "Gravity")
        self.assertEqual(rows[1]["revision_id"], 8)
        self.assertEqual(rows[0]["wikitext_sha256"],
                         hashlib.sha256(b"Force & mass").hexdigest())

    def test_rejects_multiple_revisions_duplicates_and_bad_markup(self):
        with self.assertRaisesRegex(DumpImportError, "multiple revisions"):
            self.parse(BODY.replace(b"</revision></page>", b"</revision><revision/>"
                                    b"</page>", 1))
        with self.assertRaisesRegex(DumpImportError, "duplicate page ID"):
            self.parse(BODY.replace(b"<id>43</id>", b"<id>42</id>"))
        with self.assertRaises(DumpImportError):
            self.parse(BODY[:-20])
        with self.assertRaisesRegex(DumpImportError, "DOCTYPE"):
            self.parse(b'<!DOCTYPE mediawiki [<!ENTITY x "boom">]>' + BODY)

    def test_page_and_total_budgets_fail_closed(self):
        with self.assertRaisesRegex(DumpImportError, "byte budget"):
            self.parse(max_page_bytes=10)
        with self.assertRaisesRegex(DumpImportError, "count budget"):
            self.parse(max_pages=1)

    def test_requires_current_revision_metadata(self):
        with self.assertRaisesRegex(DumpImportError, "missing or invalid"):
            self.parse(BODY.replace(b"<id>7</id>", b"<id>wrong</id>"))
        with self.assertRaisesRegex(DumpImportError, "invalid or empty"):
            self.parse(BODY.replace(b"Force &amp; mass", b""))

    def test_full_verified_bz2_input_atomically_publishes_raw_records(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            stage = root / "stage"
            stage.mkdir()
            compressed = bz2.compress(BODY)
            (stage / "articles.xml.bz2").write_bytes(compressed)
            manifest = {
                "project": "enwiki", "generation_id": "20261009",
                "source_url": "https://dumps.wikimedia.org/enwiki/20261009/dumpstatus.json",
                "completed": True,
                "files": [{
                    "name": "articles.xml.bz2", "bytes": len(compressed),
                    "sha256": hashlib.sha256(compressed).hexdigest()
                }]
            }
            out = root / "raw.jsonl"
            self.assertEqual(import_verified_member(manifest, stage, "articles.xml.bz2", out), 2)
            rows = [json.loads(x) for x in out.read_text().splitlines()]
            self.assertEqual(rows[0]["title"], "Gravity")
            with self.assertRaisesRegex(DumpImportError, "already exists"):
                import_verified_member(manifest, stage, "articles.xml.bz2", out)
            out.unlink()
            (stage / "articles.xml.bz2").write_bytes(b"bad")
            with self.assertRaises(ValueError):
                import_verified_member(manifest, stage, "articles.xml.bz2", out)
            self.assertFalse(out.exists())


if __name__ == "__main__":
    unittest.main()
