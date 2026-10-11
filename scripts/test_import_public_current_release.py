"""Synthetic end-to-end completed-public-export ingestion qualification."""
import bz2
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from discover_public_current_export import official_url
from extract_public_dump_export import DumpExtractionError
from import_public_current_release import import_official_current_release


def xml(page_id):
    return (
        f'<mediawiki><page><title>Article {page_id}</title><ns>0</ns>'
        f'<id>{page_id}</id><revision><id>{page_id + 1000}</id>'
        '<timestamp>2026-10-01T00:00:00Z</timestamp>'
        f'<text>Wiki {page_id}</text></revision></page></mediawiki>'
    ).encode()


class Response:
    status = 200

    def __init__(self, url, body):
        self.url, self.body, self.offset = url, body, 0
        self.headers = {"Content-Length": str(len(body))}

    def __enter__(self):
        return self

    def __exit__(self, *args):
        return False

    def geturl(self):
        return self.url

    def read(self, n):
        data = self.body[self.offset:self.offset + n]
        self.offset += len(data)
        return data


class Opener:
    def __init__(self, bodies):
        self.bodies = bodies
        self.calls = []

    def open(self, request, timeout):
        url = request.full_url
        self.calls.append(url)
        if url not in self.bodies:
            raise AssertionError(f"unexpected source request: {url}")
        return Response(url, self.bodies[url])


class ImportTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name)
        self.work = self.root / "work"
        self.work.mkdir()
        self.output = self.root / "pages.jsonl"
        self.url = official_url("enwiki", "2026-10-01")
        self.prefix = self.url.removesuffix("SHA256SUMS")
        self.shards = {
            "enwiki-2026-10-01-p1p100.xml.bz2": bz2.compress(xml(10)),
            "enwiki-2026-10-01-p101p200.xml.bz2": bz2.compress(xml(110)),
        }
        self.inventory = "".join(
            f"{hashlib.sha256(data).hexdigest()}  {name}\n"
            for name, data in self.shards.items()
        ).encode()
        self.bodies = {self.url: self.inventory, **{
            self.prefix + name: data for name, data in self.shards.items()
        }}

    def import_release(self, opener=None, **kwargs):
        return import_official_current_release(
            "enwiki", "2026-10-01", self.work, self.output,
            opener=opener or Opener(self.bodies), **kwargs,
        )

    def test_complete_two_shard_workflow_and_restart_reuse(self):
        source = Opener(self.bodies)
        result = self.import_release(source)
        self.assertEqual(result["unrendered_pages"], 2)
        self.assertFalse(result["offline_ready"])
        self.assertFalse(result["publisher_signature_verified"])
        self.assertEqual(result["staging"]["downloaded"], 2)
        self.assertEqual(source.calls.count(self.url), 2)
        pages = [json.loads(line) for line in self.output.read_text().splitlines()]
        self.assertEqual([page["page_id"] for page in pages], [10, 110])
        self.assertEqual([page["revision_id"] for page in pages], [1010, 1110])
        self.assertEqual([page["wikitext"] for page in pages], ["Wiki 10", "Wiki 110"])

        # An existing complete acquisition can be reprocessed to a new output
        # without downloading the compressed shards again.
        self.output = self.root / "other.jsonl"
        reused = Opener({self.url: self.inventory})
        second = self.import_release(reused)
        self.assertEqual(second["staging"]["reused_verified"], 2)
        self.assertEqual(reused.calls, [self.url])
        self.assertEqual(self.output.read_bytes(),
                         (self.root / "pages.jsonl").read_bytes())

    def test_forged_inventory_refused_before_shard_io_or_output(self):
        (self.work / "SHA256SUMS").write_bytes(self.inventory.replace(b"0", b"f", 1))
        source = Opener({self.url: self.inventory})
        with self.assertRaises(DumpExtractionError):
            self.import_release(source)
        self.assertEqual(source.calls, [self.url])
        self.assertFalse(self.output.exists())
        self.assertFalse((self.work / "shards" / next(iter(self.shards))).exists())

    def test_corrupted_second_shard_never_publishes_combined_output(self):
        bodies = dict(self.bodies)
        second = list(self.shards)[1]
        bodies[self.prefix + second] = b"bad compressed shard"
        with self.assertRaisesRegex(DumpExtractionError, "SHA-256 mismatch"):
            self.import_release(Opener(bodies))
        self.assertFalse(self.output.exists())
        first = list(self.shards)[0]
        self.assertEqual((self.work / "shards" / first).read_bytes(),
                         self.shards[first])

    def test_output_immutability_and_invalid_identity_fail_before_network(self):
        self.output.write_bytes(b"already published")
        opener = Opener({})
        with self.assertRaisesRegex(DumpExtractionError, "existing output"):
            self.import_release(opener)
        self.assertEqual(opener.calls, [])
        self.assertEqual(self.output.read_bytes(), b"already published")
        self.output.unlink()
        with self.assertRaisesRegex(DumpExtractionError, "invalid"):
            import_official_current_release(
                "not/wiki", "2026-10-01", self.work, self.output, opener=opener,
            )
        self.assertEqual(opener.calls, [])


if __name__ == "__main__":
    unittest.main()
