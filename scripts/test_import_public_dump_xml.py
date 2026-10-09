"""Synthetic streaming dump import acceptance and hostile-input regression."""
import bz2
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest

from import_public_dump_xml import (DumpImportError, import_verified_member,
                                    import_verified_members, parse_xml)

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

    def test_accepts_modern_main_slot_and_rejects_ambiguous_content(self):
        old = b'<text xml:space="preserve">Force &amp; mass</text>'
        nested = (b'<slots><slot role="main"><model>wikitext</model>'
                  b'<format>text/x-wiki</format>'
                  b'<text xml:space="preserve">Force &amp; mass</text>'
                  b'</slot></slots>')
        rows = self.parse(BODY.replace(old, nested))
        self.assertEqual(rows[0]["wikitext"], "Force & mass")
        with self.assertRaisesRegex(DumpImportError, "ambiguous"):
            self.parse(BODY.replace(old, old + nested))
        with self.assertRaisesRegex(DumpImportError, "duplicate main"):
            self.parse(BODY.replace(old, nested.replace(b'</slots>', b'') +
                                    nested.replace(b'<slots>', b'')))
        with self.assertRaisesRegex(DumpImportError, "non-main"):
            self.parse(BODY.replace(old, nested.replace(b'role="main"', b'role="aux"')))

    def test_rejects_unsupported_content_model(self):
        old = b'<text xml:space="preserve">Force &amp; mass</text>'
        bad = b'<model>json</model><format>application/json</format>' + old
        with self.assertRaisesRegex(DumpImportError, "content model"):
            self.parse(BODY.replace(old, bad))

    def test_rejects_foreign_and_missing_xml_namespaces(self):
        official = b"http://www.mediawiki.org/xml/export-0.11/"
        with self.assertRaisesRegex(DumpImportError, "official MediaWiki XML export namespace"):
            self.parse(BODY.replace(official, b"urn:attacker", 1))
        with self.assertRaisesRegex(DumpImportError, "official MediaWiki XML export namespace"):
            self.parse(BODY.replace(b' xmlns="' + official + b'"', b"", 1))
        with self.assertRaisesRegex(DumpImportError, "foreign XML namespace"):
            self.parse(BODY.replace(b"<page><title>Gravity",
                                    b'<page xmlns="urn:attacker"><title>Gravity', 1))

    def test_decoded_xml_budget_blocks_metadata_and_compression_bombs(self):
        with self.assertRaisesRegex(DumpImportError, "decoded-input byte budget"):
            self.parse(max_decoded_bytes=len(BODY) - 1)
        with self.assertRaisesRegex(DumpImportError, "invalid decoded-input"):
            self.parse(max_decoded_bytes=0)

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

    def test_import_budgets_are_explicit_and_strictly_bounded(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            stage = root / "stage"
            stage.mkdir()
            payload = bz2.compress(BODY)
            (stage / "article.xml.bz2").write_bytes(payload)
            manifest = {
                "project": "enwiki", "generation_id": "20261009",
                "source_url": "https://dumps.wikimedia.org/enwiki/20261009/dumpstatus.json",
                "completed": True,
                "files": [{
                    "name": "article.xml.bz2", "bytes": len(payload),
                    "sha256": hashlib.sha256(payload).hexdigest()
                }]
            }
            output = root / "raw.ndjson"
            for quota in [0, -1, True]:
                with self.assertRaisesRegex(DumpImportError, "budget"):
                    import_verified_members(manifest, stage, ["article.xml.bz2"], output,
                                            max_pages=quota)
            for quota in [0, -1, True, 128 * 1024 * 1024]:
                with self.assertRaisesRegex(DumpImportError, "budget"):
                    import_verified_members(manifest, stage, ["article.xml.bz2"], output,
                                            max_page_bytes=quota)
            with self.assertRaisesRegex(DumpImportError, "byte budget"):
                import_verified_members(manifest, stage, ["article.xml.bz2"], output,
                                        max_page_bytes=10)
            self.assertFalse(output.exists())
            self.assertEqual(import_verified_members(
                manifest, stage, ["article.xml.bz2"], output,
                max_pages=10_000_000, max_page_bytes=32 * 1024 * 1024
            ), 2)

    def test_requires_utc_revision_timestamp_and_unambiguous_redirect(self):
        for value in [b"not-a-time", b"2026-02-30T00:00:00Z",
                      b"2026-10-09T00:00:00+04:00", b"2026-10-09"]:
            with self.assertRaisesRegex(DumpImportError, "UTC revision timestamp"):
                self.parse(BODY.replace(b"2026-10-09T00:00:00Z", value, 1))
        with self.assertRaisesRegex(DumpImportError, "no target"):
            self.parse(BODY.replace(b'<redirect title="Gravity"/>',
                                    b'<redirect title="   "/>'))
        with self.assertRaisesRegex(DumpImportError, "duplicate redirect"):
            self.parse(BODY.replace(b'<redirect title="Gravity"/>',
                                    b'<redirect title="Gravity"/>'
                                    b'<redirect title="Other"/>'))

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
            self.assertEqual(rows[0]["source_member"], "articles.xml.bz2")
            self.assertEqual(rows[0]["source_member_sha256"],
                             hashlib.sha256(compressed).hexdigest())
            self.assertEqual(
                rows[0]["source_member_url"],
                "https://dumps.wikimedia.org/enwiki/20261009/articles.xml.bz2",
            )
            with self.assertRaisesRegex(DumpImportError, "already exists"):
                import_verified_member(manifest, stage, "articles.xml.bz2", out)
            out.unlink()
            (stage / "articles.xml.bz2").write_bytes(b"bad")
            with self.assertRaises(ValueError):
                import_verified_member(manifest, stage, "articles.xml.bz2", out)
            self.assertFalse(out.exists())


    def test_modern_sha256_export_records_exact_published_member_url(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            stage = root / "stage"
            stage.mkdir()
            name = "enwiki-2026-10-01-p1p2.xml.bz2"
            raw = bz2.compress(BODY)
            (stage / name).write_bytes(raw)
            root_url = (
                "https://dumps.wikimedia.org/other/mediawiki_content_current/"
                "enwiki/2026-10-01/xml/bzip2/"
            )
            manifest = {
                "project": "enwiki", "generation_id": "2026-10-01",
                "source_url": root_url + "SHA256SUMS",
                "completed": True,
                "files": [{
                    "name": name, "bytes": len(raw),
                    "sha256": hashlib.sha256(raw).hexdigest(),
                    "relative_path": "shards/" + name,
                    "url": root_url + "shards/" + name,
                }],
            }
            output = root / "modern.jsonl"
            self.assertEqual(import_verified_members(manifest, stage, [name], output), 2)
            records = [json.loads(x) for x in output.read_text().splitlines()]
            self.assertEqual(records[0]["source_member_url"], root_url + "shards/" + name)
            self.assertEqual(records[0]["source_member_sha256"],
                             hashlib.sha256(raw).hexdigest())
            output.unlink()
            for wrong in (root_url + name, "https://evil.test/" + name):
                manifest["files"][0]["url"] = wrong
                with self.assertRaisesRegex(DumpImportError, "URL mismatches"):
                    import_verified_members(manifest, stage, [name], output)
                self.assertFalse(output.exists())
            manifest["files"][0]["url"] = root_url + "shards/" + name
            manifest["files"][0]["relative_path"] = "../" + name
            with self.assertRaisesRegex(DumpImportError, "unsafe modern"):
                import_verified_members(manifest, stage, [name], output)
            self.assertFalse(output.exists())

    def test_verified_xml_shards_combine_atomically_without_cross_member_duplicates(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            stage = root / "stage"
            stage.mkdir()
            split = BODY.index(b"<page><title>Redirect")
            first = BODY[:split] + b"</mediawiki>"
            second = BODY[:BODY.index(b"<page>")] + BODY[split:]
            files = []
            for name, body in (("part-1.xml.bz2", first), ("part-2.xml.bz2", second)):
                data = bz2.compress(body)
                (stage / name).write_bytes(data)
                files.append({"name": name, "bytes": len(data),
                              "sha256": hashlib.sha256(data).hexdigest()})
            manifest = {
                "project": "enwiki", "generation_id": "20261009",
                "source_url": "https://dumps.wikimedia.org/enwiki/20261009/dumpstatus.json",
                "completed": True, "files": files,
            }
            names = [entry["name"] for entry in files]
            out = root / "combined.jsonl"
            self.assertEqual(import_verified_members(manifest, stage, names, out), 2)
            rows = [json.loads(line) for line in out.read_text().splitlines()]
            self.assertEqual([row["page_id"] for row in rows], [42, 43])
            self.assertEqual({row["generation_id"] for row in rows}, {"20261009"})
            self.assertEqual(
                [row["source_member"] for row in rows],
                ["part-1.xml.bz2", "part-2.xml.bz2"],
            )
            reversed_out = root / "reversed.jsonl"
            self.assertEqual(import_verified_members(
                manifest, stage, list(reversed(names)), reversed_out), 2)
            self.assertEqual(out.read_bytes(), reversed_out.read_bytes())
            with self.assertRaisesRegex(DumpImportError, "already exists"):
                import_verified_members(manifest, stage, names, out)
            out.unlink()

            with self.assertRaisesRegex(DumpImportError, "duplicate XML member"):
                import_verified_members(manifest, stage, [names[0]] * 2, out)
            self.assertFalse(out.exists())
            with self.assertRaisesRegex(DumpImportError, "combined page count"):
                import_verified_members(manifest, stage, names, out, max_pages=1)
            self.assertFalse(out.exists())
            with self.assertRaisesRegex(DumpImportError, "decoded-input byte budget"):
                import_verified_members(
                    manifest, stage, names, out,
                    max_decoded_bytes=len(first) + len(second) - 1,
                )
            self.assertFalse(out.exists())
            with self.assertRaisesRegex(DumpImportError, "not a verified XML"):
                import_verified_members(manifest, stage, ["unknown.xml"], out)
            self.assertFalse(out.exists())

            # Both files individually valid, but combining overlapping pages
            # must never publish an apparently complete snapshot.
            overlap = bz2.compress(first)
            (stage / names[1]).write_bytes(overlap)
            manifest["files"][1]["bytes"] = len(overlap)
            manifest["files"][1]["sha256"] = hashlib.sha256(overlap).hexdigest()
            with self.assertRaisesRegex(DumpImportError, "duplicate page ID across"):
                import_verified_members(manifest, stage, names, out)
            self.assertFalse(out.exists())


if __name__ == "__main__":
    unittest.main()
