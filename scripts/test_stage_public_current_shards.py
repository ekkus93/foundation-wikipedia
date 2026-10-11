"""Acceptance tests for verified current-content shard staging."""
import hashlib
from pathlib import Path
import tempfile
import unittest

from stage_public_current_shards import stage_verified_members


class FakeResponse:
    status = 200

    def __init__(self, url, data):
        self.url, self.data, self.offset = url, data, 0

    def __enter__(self):
        return self

    def __exit__(self, *args):
        return False

    def geturl(self):
        return self.url

    def read(self, size):
        chunk = self.data[self.offset:self.offset + size]
        self.offset += len(chunk)
        return chunk


class FakeOpener:
    def __init__(self, data):
        self.data = data
        self.calls = 0

    def open(self, request, timeout):
        self.calls += 1
        return FakeResponse(request.full_url, self.data)


class StagingTests(unittest.TestCase):
    def test_download_and_reuse(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            stage = root / "stage"
            stage.mkdir()
            inventory = root / "SHA256SUMS"
            name = "enwiki-2026-10-01-p1p100.xml.bz2"
            data = b"example compressed shard"
            inventory.write_text(hashlib.sha256(data).hexdigest() + "  " + name + "\n")
            first = FakeOpener(data)
            result = stage_verified_members(inventory, "enwiki", "2026-10-01",
                                            stage, opener=first)
            self.assertEqual(result["downloaded"], 1)
            self.assertEqual((stage / name).read_bytes(), data)
            second = FakeOpener(data)
            result = stage_verified_members(inventory, "enwiki", "2026-10-01",
                                            stage, opener=second)
            self.assertEqual(result["reused_verified"], 1)
            self.assertEqual(second.calls, 0)


if __name__ == "__main__":
    unittest.main()
