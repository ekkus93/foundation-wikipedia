"""Acceptance and negative transport tests for current-content shard staging."""
import hashlib
from pathlib import Path
import tempfile
import unittest
from urllib.error import URLError

from extract_public_dump_export import DumpExtractionError
from stage_public_current_shards import (stage_verified_members,
                                         stage_official_verified_members)


class FakeResponse:
    status = 200

    def __init__(self, url, data, *, status=200):
        self.url, self.data, self.offset, self.status = url, data, 0, status
        self.headers = {}

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


    def test_interrupted_transfer_resumes_exact_bytes(self):
        from urllib.error import URLError

        class BrokenResponse(FakeResponse):
            def read(self, size):
                if self.offset == 6:
                    raise URLError("simulated connection loss")
                chunk = self.data[self.offset:6]
                self.offset += len(chunk)
                return chunk

        class BrokenOpener:
            def open(self, request, timeout):
                return BrokenResponse(request.full_url, self_data)

        self_data = self.data
        with self.assertRaisesRegex(DumpExtractionError, "HTTPS failure"):
            self.stage_members(BrokenOpener())
        part = self.stage / (self.name + ".part")
        self.assertEqual(part.read_bytes(), self.data[:6])
        self.assertFalse((self.stage / self.name).exists())

        observed = []

        class ResumeOpener:
            def open(self, request, timeout):
                observed.append(request.get_header("Range"))
                return RangeResponse(request.full_url, self_data[6:],
                                     "bytes 6-" + str(len(self_data) - 1) +
                                     "/" + str(len(self_data)))

        class RangeResponse(FakeResponse):
            def __init__(self, url, data, content_range):
                super().__init__(url, data, status=206)
                self.headers = {
                    "Content-Range": content_range,
                    "Content-Length": str(len(data)),
                }

        result = self.stage_members(ResumeOpener())
        self.assertEqual(observed, ["bytes=6-"])
        self.assertEqual(result["resumed"], 1)
        self.assertEqual((self.stage / self.name).read_bytes(), self.data)
        self.assertFalse(part.exists())

    def test_resume_rejects_incorrect_or_ignored_ranges(self):
        part = self.stage / (self.name + ".part")
        for response_status, range_header in (
            (200, ""),
            (206, ""),
            (206, "bytes 0-10/11"),
            (206, "bytes 6-100/101"),
        ):
            with self.subTest(status=response_status, range=range_header):
                part.write_bytes(self.data[:6])

                class BadResume:
                    def open(self, request, timeout):
                        self_request_range = request.get_header("Range")
                        self_outer.assertEqual(self_request_range, "bytes=6-")
                        response = FakeResponse(request.full_url,
                                                self_data[6:],
                                                status=response_status)
                        response.headers = {"Content-Range": range_header}
                        return response

                self_outer, self_data = self, self.data
                with self.assertRaises(DumpExtractionError):
                    self.stage_members(BadResume())
                self.assertTrue(part.read_bytes().startswith(self.data[:6]))
                self.assertFalse((self.stage / self.name).exists())

    def test_complete_partial_promotes_without_network(self):
        part = self.stage / (self.name + ".part")
        part.write_bytes(self.data)
        result = self.stage_members(FakeOpener(self.data, fail=True))
        self.assertEqual(result["resumed"], 1)
        self.assertEqual((self.stage / self.name).read_bytes(), self.data)
        self.assertFalse(part.exists())

    def test_symlinked_interrupted_shard_is_rejected(self):
        part = self.stage / (self.name + ".part")
        real = self.root / "other"
        real.write_bytes(self.data[:6])
        part.symlink_to(real)
        with self.assertRaisesRegex(DumpExtractionError, "unsafe interrupted shard"):
            self.stage_members(FakeOpener(self.data))
        self.assertFalse((self.stage / self.name).exists())
        self.assertEqual(real.read_bytes(), self.data[:6])


    def test_official_preflight_matches_publication_before_any_shard_requests(self):
        seen = []
        inventory_bytes = self.inventory.read_bytes()
        local_data = self.data

        class OfficialOpener:
            def open(self, request, timeout):
                seen.append(request.full_url)
                if request.full_url.endswith("/SHA256SUMS"):
                    return FakeResponse(request.full_url, inventory_bytes)
                return FakeResponse(request.full_url, local_data)

        result = stage_official_verified_members(
            self.inventory, "enwiki", "2026-10-01",
            self.stage, opener=OfficialOpener(),
        )
        self.assertEqual(result["downloaded"], 1)
        self.assertEqual(len(seen), 2)
        self.assertTrue(seen[0].endswith("/SHA256SUMS"))
        self.assertEqual((self.stage / self.name).read_bytes(), self.data)

    def test_official_preflight_rejects_forged_or_unavailable_publication(self):
        observed = []
        inventory_bytes = self.inventory.read_bytes()

        class ForgedOfficialOpener:
            def __init__(self, *, redirect=False, unavailable=False):
                self.redirect, self.unavailable = redirect, unavailable

            def open(self, request, timeout):
                observed.append(request.full_url)
                if not request.full_url.endswith("/SHA256SUMS"):
                    self_outer.fail("shard download attempted after rejected publication")
                if self.unavailable:
                    return FakeResponse(request.full_url, inventory_bytes, status=404)
                if self.redirect:
                    return FakeResponse("https://mirror.invalid/SHA256SUMS",
                                        inventory_bytes)
                return FakeResponse(request.full_url, b"forged SHA256SUMS")

        self_outer = self
        for options in ({}, {"redirect": True}, {"unavailable": True}):
            with self.subTest(options=options):
                observed.clear()
                with self.assertRaises(DumpExtractionError):
                    stage_official_verified_members(
                        self.inventory, "enwiki", "2026-10-01",
                        self.stage, opener=ForgedOfficialOpener(**options),
                    )
                self.assertEqual(len(observed), 1)
                self.assertFalse((self.stage / self.name).exists())


if __name__ == "__main__":
    unittest.main()
