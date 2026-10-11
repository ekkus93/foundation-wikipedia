"""Offline tests for official completed current-content inventory discovery."""
import hashlib
from pathlib import Path
import tempfile
import unittest
from urllib.error import HTTPError

from discover_public_current_export import (
    DumpExtractionError, discover_completed_export, official_url,
)


class Response:
    def __init__(self, url, payload, status=200):
        self.url, self.payload, self.status = url, payload, status

    def __enter__(self):
        return self

    def __exit__(self, *args):
        return False

    def geturl(self):
        return self.url

    def read(self, size):
        return self.payload[:size]


class Opener:
    def __init__(self, response=None, error=None):
        self.response, self.error = response, error
        self.calls = []

    def open(self, request, timeout):
        self.calls.append((request.full_url, timeout))
        if self.error is not None:
            raise self.error
        return self.response


class DiscoveryTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name)
        self.output = self.root / "SHA256SUMS"
        self.url = official_url("enwiki", "2026-10-01")
        self.payload = (("a" * 64) +
                        "  enwiki-2026-10-01-p1p100.xml.bz2\n").encode()

    def discover(self, response=None, error=None):
        opener = Opener(response or Response(self.url, self.payload), error)
        result = discover_completed_export(
            "enwiki", "2026-10-01", self.output, opener=opener,
        )
        self.assertEqual(opener.calls, [(self.url, 30)])
        return result

    def assert_not_published(self):
        self.assertFalse(self.output.exists())
        self.assertFalse(list(self.root.glob(".wikimedia-inventory.*")))

    def test_completed_official_inventory_published_once(self):
        result = self.discover()
        self.assertEqual(self.output.read_bytes(), self.payload)
        self.assertEqual(result["sha256"], hashlib.sha256(self.payload).hexdigest())
        self.assertEqual(result["members"], 1)
        self.assertFalse(result["shards_verified"])
        with self.assertRaisesRegex(DumpExtractionError, "existing"):
            self.discover()

    def test_rejects_wrong_origin_and_unready_publication(self):
        with self.assertRaisesRegex(DumpExtractionError, "official response"):
            self.discover(Response("https://mirror.example/SHA256SUMS", self.payload))
        self.assert_not_published()
        error = HTTPError(self.url, 404, "Not Found", {}, None)
        with self.assertRaisesRegex(DumpExtractionError, "not completed"):
            self.discover(error=error)
        self.assert_not_published()

    def test_rejects_invalid_and_oversized_inventories(self):
        for payload in (b"", b"bad checksum", self.payload + b"x" * (32 * 1024 * 1024)):
            with self.subTest(size=len(payload)):
                with self.assertRaises(DumpExtractionError):
                    self.discover(Response(self.url, payload))
                self.assert_not_published()

    def test_invalid_project_or_month_never_contacts_network(self):
        for project, date in (("../enwiki", "2026-10-01"),
                              ("enwiki", "2026-02-01x"),
                              ("enwiki", "2026-13-01"),
                              ("enwiki", "2026-10-02")):
            with self.subTest(project=project, date=date):
                with self.assertRaises(DumpExtractionError):
                    discover_completed_export(
                        project, date, self.output, opener=Opener(),
                    )
                self.assert_not_published()


if __name__ == "__main__":
    unittest.main()
