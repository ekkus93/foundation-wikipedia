"""Synthetic official inventory -> verified shards -> raw revision JSONL."""
import bz2
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from discover_public_current_export import discover_completed_export, official_url
from extract_public_dump_export import extract_current_export
from test_discover_public_current_export import Response as InventoryResponse
from test_discover_public_current_export import Opener as InventoryOpener
from test_stage_public_current_shards import FakeResponse
from stage_public_current_shards import stage_verified_members


class ShardOpener:
    def __init__(self, payloads):
        self.payloads = payloads

    def open(self, request, timeout):
        return FakeResponse(request.full_url, self.payloads[request.full_url])


def page_xml(page_id):
    return (
        '<?xml version="1.0" encoding="utf-8"?>'
        '<mediawiki><page><title>Article ' + str(page_id) + '</title>'
        '<ns>0</ns><id>' + str(page_id) + '</id><revision><id>'
        + str(page_id + 1000) + '</id>'
        '<timestamp>2026-10-01T00:00:00Z</timestamp>'
        '<text>Example ' + str(page_id) + '</text>'
        '</revision></page></mediawiki>'
    ).encode()


class PipelineTests(unittest.TestCase):
    def test_two_shard_verified_pipeline_keeps_exact_revisions(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            stage = root / "stage"
            stage.mkdir()
            inventory = root / "SHA256SUMS"
            output = root / "unrendered.jsonl"
            members = {
                "enwiki-2026-10-01-p1p100.xml.bz2": bz2.compress(page_xml(10)),
                "enwiki-2026-10-01-p101p200.xml.bz2": bz2.compress(page_xml(110)),
            }
            payload = "".join(
                hashlib.sha256(data).hexdigest() + "  " + name + "\n"
                for name, data in members.items()
            ).encode()
            url = official_url("enwiki", "2026-10-01")
            discovery = discover_completed_export(
                "enwiki", "2026-10-01", inventory,
                opener=InventoryOpener(InventoryResponse(url, payload)),
            )
            self.assertEqual(discovery["members"], 2)
            self.assertFalse(discovery["shards_verified"])
            prefix = url.removesuffix("SHA256SUMS")
            staged = stage_verified_members(
                inventory, "enwiki", "2026-10-01", stage,
                opener=ShardOpener({prefix + name: data for name, data in members.items()}),
            )
            self.assertEqual(staged["downloaded"], 2)
            self.assertEqual(extract_current_export(
                stage, inventory, "enwiki", "2026-10-01", output,
            ), 2)
            rows = [json.loads(line) for line in output.read_text().splitlines()]
            self.assertEqual([row["page_id"] for row in rows], [10, 110])
            self.assertEqual([row["revision_id"] for row in rows], [1010, 1110])
            self.assertEqual([row["wikitext"] for row in rows],
                             ["Example 10", "Example 110"])


if __name__ == "__main__":
    unittest.main()
