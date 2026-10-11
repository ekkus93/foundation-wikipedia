"""Acceptance and negative transport tests for current-content shard staging."""
import hashlib
from pathlib import Path
import tempfile
import unittest
from urllib.error import URLError

from extract_public_dump_export import DumpExtractionError
from stage_public_current_shards import stage_verified_members


class FakeResponse:
    status = 200

    def __init__(self, url, data, *, status=200):
        self.url, self.data, self.offset, self.status = url, data, 0, status

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
    def __init__(self, data, *, status=200, redirect=None, fail=False):
        self.data, self.status, self.redirect, self.fail = (
            data, status, redirect, fail
        )
        self.calls = 0

    def open(self, request, timeout):
        self.calls += 1
        if self.fail:
            raise URLError("simulated upstream connection loss")
        return FakeResponse(self.redirect or request.full_url, self.data,
                            status=self.status)


class StagingTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.stage = self.root / "stage"
        self.stage.mkdir()
        self.inventory = self.root / "SHA256SUMS"
        self.name = "enwiki-2026-10-01-p1p100.xml.bz2"
        self.data = b"example compressed shard"
        self.inventory.write_text(
            hashlib.sha256(self.data).hexdigest() + "  " + self.name + "\n"
        )

    def stage_members(self, opener, **kwargs):
        return stage_verified_members(
            self.inventory, "enwiki", "2026-10-01",
            self.stage, opener=opener, **kwargs
        )

    def test_download_and_restart_reuse(self):
        first = FakeOpener(self.data)
        result = self.stage_members(first)
        self.assertEqual(result["downloaded"], 1)
        self.assertEqual(result["compressed_bytes"], len(self.data))
        self.assertEqual((self.stage / self.name).read_bytes(), self.data)
        second = FakeOpener(self.data, fail=True)
        result = self.stage_members(second)
        self.assertEqual(result["reused_verified"], 1)
        self.assertEqual(second.calls, 0)

    def test_corrupted_transport_never_publishes_target(self):
        with self.assertRaisesRegex(DumpExtractionError, "SHA-256 mismatch"):
            self.stage_members(FakeOpener(b"tampered"))
        self.assertFalse((self.stage / self.name).exists())
        self.assertEqual(list(self.stage.glob(".wiki-shard.*")), [])

    def test_redirect_and_wrong_status_are_rejected(self):
        for opener in (
            FakeOpener(self.data, redirect="https://other.example/shard"),
            FakeOpener(self.data, status=206),
            FakeOpener(self.data, status=404),
        ):
            with self.subTest(status=opener.status, redirect=opener.redirect):
                with self.assertRaisesRegex(DumpExtractionError,
                                            "unexpected official shard response"):
                    self.stage_members(opener)
                self.assertFalse((self.stage / self.name).exists())
                self.assertEqual(list(self.stage.glob(".wiki-shard.*")), [])

    def test_member_and_total_budgets_fail_closed(self):
        for budget in (
            {"max_member_bytes": len(self.data) - 1},
            {"max_total_bytes": len(self.data) - 1},
        ):
            with self.subTest(budget=budget):
                with self.assertRaisesRegex(DumpExtractionError,
                                            "download budget"):
                    self.stage_members(FakeOpener(self.data), **budget)
                self.assertFalse((self.stage / self.name).exists())
                self.assertEqual(list(self.stage.glob(".wiki-shard.*")), [])

    def test_transport_failure_never_publishes_member(self):
        with self.assertRaisesRegex(DumpExtractionError, "HTTPS failure"):
            self.stage_members(FakeOpener(self.data, fail=True))
        self.assertFalse((self.stage / self.name).exists())
        self.assertEqual(list(self.stage.glob(".wiki-shard.*")), [])

    def test_corrupted_existing_file_is_never_reused(self):
        (self.stage / self.name).write_bytes(b"forged")
        opener = FakeOpener(self.data)
        with self.assertRaises(DumpExtractionError):
            self.stage_members(opener)
        self.assertEqual(opener.calls, 0)
        self.assertEqual((self.stage / self.name).read_bytes(), b"forged")


if __name__ == "__main__":
    unittest.main()
