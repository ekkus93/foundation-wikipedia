"""Offline end-to-end staging fixtures for the completed current-content export."""
import hashlib
import bz2
from extract_public_dump_export import extract_current_export
import io
import json
from pathlib import Path
import tempfile
import unittest

from discover_public_dump_inventory import BASE, PublicationError
from stage_public_dump_export import stage_completed_export


class StageTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.target = self.root / "staged"
        self.date = "2026-10-01"
        self.a = f"enwiki-{self.date}-p1p100.xml.bz2"
        self.b = f"enwiki-{self.date}-p101p200.xml.bz2"
        self.payloads = {self.a: b"first synthetic shard",
                         self.b: b"second synthetic shard"}

    def fetch(self, url, limit):
        if url == BASE + "enwiki/":
            return b'<a href="2026-10-01/">Complete</a>'
        if url.endswith("SHA256SUMS"):
            return "".join(
                f"{hashlib.sha256(data).hexdigest()}  {name}\n"
                for name, data in self.payloads.items()
            ).encode()
        raise AssertionError(url)

    def stream(self, url):
        self.assertTrue(url.startswith(BASE + "enwiki/2026-10-01/xml/bzip2/"))
        return io.BytesIO(self.payloads[url.rsplit("/", 1)[-1]])

    def stage(self, **kwargs):
        return stage_completed_export("enwiki", self.target,
                                      fetch=self.fetch, stream=self.stream, **kwargs)

    def test_two_shards_and_inventory_publish_as_one_directory(self):
        receipt = self.stage()
        self.assertEqual(receipt["members"], 2)
        self.assertEqual(receipt["compressed_bytes"], sum(map(len, self.payloads.values())))
        for name, data in self.payloads.items():
            self.assertEqual((self.target / name).read_bytes(), data)
        self.assertEqual(json.loads((self.target / "PUBLICATION.json").read_text()),
                         receipt)
        self.assertIn(self.a, (self.target / "SHA256SUMS").read_text())
        with self.assertRaisesRegex(PublicationError, "already exists"):
            self.stage()

    def test_offline_staging_to_complete_revision_jsonl_import(self):
        def xml(page_id):
            return (f'<?xml version="1.0" encoding="utf-8"?>'
                    f'<mediawiki><page><title>Article {page_id}</title><ns>0</ns>'
                    f'<id>{page_id}</id><revision><id>{page_id + 1000}</id>'
                    f'<timestamp>2026-10-01T00:00:00Z</timestamp>'
                    f'<text>Wiki {page_id}</text></revision></page></mediawiki>').encode()

        self.payloads = {self.a: bz2.compress(xml(10)),
                         self.b: bz2.compress(xml(110))}
        receipt = self.stage()
        output = self.root / "revisions.jsonl"
        self.assertEqual(
            extract_current_export(self.target, self.target / "SHA256SUMS",
                                   "enwiki", self.date, output), 2
        )
        entries = [json.loads(line) for line in output.read_text().splitlines()]
        self.assertEqual([entry["page_id"] for entry in entries], [10, 110])
        self.assertEqual(receipt["members"], 2)

    def test_in_tree_publication_receipt_must_match_checksum_identity(self):
        def xml(page_id):
            return (f'<mediawiki><page><title>Article {page_id}</title><ns>0</ns>'
                    f'<id>{page_id}</id><revision><id>{page_id + 1000}</id>'
                    '<timestamp>2026-10-01T00:00:00Z</timestamp>'
                    f'<text>Wiki {page_id}</text></revision></page></mediawiki>').encode()

        self.payloads = {self.a: bz2.compress(xml(10)),
                         self.b: bz2.compress(xml(110))}
        self.stage()
        original = json.loads((self.target / "PUBLICATION.json").read_text())
        output = self.root / "revisions.jsonl"
        for variant in ("date", "member_count", "sha256", "compressed_bytes", "missing"):
            with self.subTest(variant=variant):
                data = json.loads(json.dumps(original))
                if variant == "date":
                    data["publication"]["date"] = "2026-09-01"
                elif variant == "member_count":
                    data["members"] += 1
                elif variant == "sha256":
                    data["publication"]["sha256"] = "0" * 64
                elif variant == "compressed_bytes":
                    data["compressed_bytes"] += 1
                else:
                    data = {"publication": "forged"}
                (self.target / "PUBLICATION.json").write_text(json.dumps(data))
                with self.assertRaisesRegex(
                    ValueError, "staged publication|invalid staged publication",
                ):
                    extract_current_export(
                        self.target, self.target / "SHA256SUMS",
                        "enwiki", self.date, output,
                    )
                self.assertFalse(output.exists())
        (self.target / "PUBLICATION.json").write_text(json.dumps(original))
        self.assertEqual(
            extract_current_export(
                self.target, self.target / "SHA256SUMS",
                "enwiki", self.date, output,
            ), 2,
        )

    def test_mutated_receipt_after_shard_parsing_never_publishes(self):
        from unittest.mock import patch
        import extract_public_dump_export as module

        def xml(page_id):
            return (f'<mediawiki><page><title>Article {page_id}</title><ns>0</ns>'
                    f'<id>{page_id}</id><revision><id>{page_id + 1000}</id>'
                    '<timestamp>2026-10-01T00:00:00Z</timestamp>'
                    f'<text>Wiki {page_id}</text></revision></page></mediawiki>').encode()

        self.payloads = {self.a: bz2.compress(xml(10)),
                         self.b: bz2.compress(xml(110))}
        self.stage()
        original = module.extract_current_dump
        output = self.root / "revisions.jsonl"

        def mutate_after_final_shard(*args, **kwargs):
            count = original(*args, **kwargs)
            if args[0].name == self.b:
                receipt_path = self.target / "PUBLICATION.json"
                receipt = json.loads(receipt_path.read_text())
                receipt["compressed_bytes"] += 1
                receipt_path.write_text(json.dumps(receipt))
            return count

        with patch.object(module, "extract_current_dump",
                          side_effect=mutate_after_final_shard):
            with self.assertRaisesRegex(
                ValueError, "staged publication compressed byte count mismatch",
            ):
                extract_current_export(
                    self.target, self.target / "SHA256SUMS",
                    "enwiki", self.date, output,
                )
        self.assertFalse(output.exists())

    def test_corrupted_later_member_does_not_publish_partial_directory(self):
        def corrupted(url):
            if url.endswith(self.b):
                return io.BytesIO(b"wrong")
            return self.stream(url)

        with self.assertRaisesRegex(PublicationError, "SHA-256 mismatch"):
            stage_completed_export("enwiki", self.target,
                                   fetch=self.fetch, stream=corrupted)
        self.assertFalse(self.target.exists())
        self.assertFalse(list(self.root.glob(".current-export.*")))

    def test_aggregate_and_member_budgets_fail_closed(self):
        for limits in ({"max_total_bytes": len(self.payloads[self.a])},
                       {"max_member_bytes": 5},
                       {"max_total_bytes": 0}):
            with self.subTest(limits=limits):
                with self.assertRaises(PublicationError):
                    self.stage(**limits)
                self.assertFalse(self.target.exists())
                self.assertFalse(list(self.root.glob(".current-export.*")))


if __name__ == "__main__":
    unittest.main()
