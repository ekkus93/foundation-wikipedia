"""Offline transport regressions for a complete current-content export."""
import copy
import hashlib
import io
import os
import json
from pathlib import Path
import tempfile
import unittest
from urllib.error import HTTPError
from unittest.mock import patch

from discover_current_content_export import parse_checksums
from stage_current_content_export import ExportStagingError, stage_export

URL = "https://dumps.wikimedia.org/other/mediawiki_content_current/enwiki/2026-10-01/xml/bzip2/"
BODY_A = b"modern XML bytes A"
BODY_B = b"modern XML bytes B"
NAMES = ("enwiki-2026-10-01-p1p2.xml.bz2", "enwiki-2026-10-01-p3p4.xml.bz2")
SUMS = "".join(
    f"{hashlib.sha256(body).hexdigest()}  {name}\n"
    for name, body in zip(NAMES, (BODY_A, BODY_B))
)


class Response(io.BytesIO):
    def __init__(self, data, url, status=200, headers=None):
        super().__init__(data)
        self.url, self.status, self.headers = url, status, headers or {}

    def geturl(self):
        return self.url


class ModernExportStagingTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.dest = self.root / "staging"
        self.output = self.root / "manifest.json"
        self.report = parse_checksums(SUMS, "enwiki", "2026-10-01")
        self.requests = []

    def fetch(self, url):
        self.assertEqual(url, URL + "SHA256SUMS")
        return SUMS

    def opener(self, request, timeout):
        self.assertEqual(timeout, 40)
        url = request.full_url
        name = url.rsplit("/", 1)[-1]
        payload = dict(zip(NAMES, (BODY_A, BODY_B)))[name]
        rng = request.get_header("Range")
        self.requests.append((name, rng))
        if rng is not None:
            offset = int(rng.removeprefix("bytes=").removesuffix("-"))
            return Response(payload[offset:], url, 206, {
                "Content-Range": f"bytes {offset}-{len(payload)-1}/{len(payload)}",
                "Content-Length": str(len(payload) - offset),
            })
        return Response(payload, url, headers={"Content-Length": str(len(payload))})

    def stage(self, *, opener=None, report=None):
        return stage_export(
            report or self.report, self.dest, self.output,
            fetcher=self.fetch, opener=opener or self.opener
        )

    def test_complete_export_is_atomically_published(self):
        result = self.stage()
        self.assertEqual(result["source_url"], URL + "SHA256SUMS")
        self.assertEqual(
            result["files"][0]["url"],
            URL + result["files"][0]["name"],
        )
        self.assertEqual(result["files"][0]["relative_path"], result["files"][0]["name"])
        self.assertEqual(len(result["files"]), 2)
        self.assertEqual(json.loads(self.output.read_text()), result)
        for item in result["files"]:
            self.assertEqual(
                hashlib.sha256((self.dest / item["name"]).read_bytes()).hexdigest(),
                item["sha256"]
            )
        self.assertEqual(len(self.requests), 2)

    def test_interrupted_resume_uses_exact_range(self):
        self.dest.mkdir()
        (self.dest / (NAMES[0] + ".part")).write_bytes(BODY_A[:6])
        result = self.stage()
        self.assertEqual(result["files"][0]["bytes"], len(BODY_A))
        self.assertIn((NAMES[0], "bytes=6-"), self.requests)

    def test_complete_partial_promotes_without_an_extra_request(self):
        self.dest.mkdir()
        (self.dest / (NAMES[0] + ".part")).write_bytes(BODY_A)
        self.stage()
        self.assertNotIn(NAMES[0], [name for name, _ in self.requests])
        self.assertEqual((self.dest / NAMES[0]).read_bytes(), BODY_A)

    def test_empty_partial_restarts_safely(self):
        self.dest.mkdir()
        (self.dest / (NAMES[0] + ".part")).touch()
        self.stage()
        self.assertEqual((self.dest / NAMES[0]).read_bytes(), BODY_A)

    def test_explicit_mirror_uses_pinned_official_sha256_and_no_redirects(self):
        mirror = "https://mirror.example.test/wikimedia/"
        observed = []

        def mirrored_response(request, timeout):
            observed.append(request.full_url)
            name = request.full_url.rsplit("/", 1)[-1]
            payload = dict(zip(NAMES, (BODY_A, BODY_B)))[name]
            return Response(payload, request.full_url,
                            headers={"Content-Length": str(len(payload))})

        result = stage_export(
            self.report, self.dest, self.output, fetcher=self.fetch,
            opener=mirrored_response, mirror_base=mirror,
        )
        self.assertEqual(observed, [mirror + name for name in NAMES])
        self.assertEqual(result["files"][0]["url"], URL + NAMES[0])
        self.assertFalse(any(mirror in str(entry) for entry in result["files"]))

    def test_local_file_source_uses_official_inventory_and_leaves_http_partials(self):
        source = self.root / "local-files"
        source.mkdir()
        for name, body in zip(NAMES, (BODY_A, BODY_B)):
            (source / name).write_bytes(body)
        self.dest.mkdir()
        partial = self.dest / (NAMES[0] + ".part")
        partial.write_bytes(b"do not touch network partial")
        result = stage_export(
            self.report, self.dest, self.output, fetcher=self.fetch,
            opener=lambda *_args, **_kwargs: self.fail("local mode made download"),
            local_directory=source,
        )
        self.assertEqual(len(result["files"]), 2)
        self.assertEqual(partial.read_bytes(), b"do not touch network partial")
        self.assertEqual((self.dest / NAMES[1]).read_bytes(), BODY_B)

    def test_local_corruption_and_symlinks_fail_without_manifest(self):
        source = self.root / "local-files"
        source.mkdir()
        (source / NAMES[0]).write_bytes(b"changed")
        (source / NAMES[1]).write_bytes(BODY_B)
        with self.assertRaisesRegex(ExportStagingError, "SHA-256 mismatch"):
            stage_export(self.report, self.dest, self.output, fetcher=self.fetch,
                         local_directory=source)
        self.assertFalse(self.output.exists())
        (source / NAMES[0]).unlink()
        (source / NAMES[0]).symlink_to(source / NAMES[1])
        with self.assertRaisesRegex(ExportStagingError, "unsafe local member"):
            stage_export(self.report, self.dest, self.output, fetcher=self.fetch,
                         local_directory=source)
        self.assertFalse(self.output.exists())

    def test_invalid_local_directory_and_conflicting_modes_fail_preflight(self):
        missing = self.root / "missing"
        with self.assertRaisesRegex(ExportStagingError, "source directory"):
            stage_export(self.report, self.dest, self.output,
                         fetcher=lambda _url: self.fail("early source lookup"),
                         local_directory=missing)
        with self.assertRaisesRegex(ExportStagingError, "mutually exclusive"):
            stage_export(self.report, self.dest, self.output,
                         fetcher=lambda _url: self.fail("early source lookup"),
                         local_directory=missing, mirror_base="https://mirror.example.test/")

    def test_mirror_corruption_rejected_and_no_manifest(self):
        mirror = "https://mirror.example.test/"

        def corrupt(request, timeout):
            name = request.full_url.rsplit("/", 1)[-1]
            body = b"bad mirrors" if name == NAMES[0] else BODY_B
            return Response(body, request.full_url,
                            headers={"Content-Length": str(len(body))})

        with self.assertRaisesRegex(ExportStagingError, "SHA-256 mismatch"):
            stage_export(
                self.report, self.dest, self.output, fetcher=self.fetch,
                opener=corrupt, mirror_base=mirror,
            )
        self.assertFalse(self.output.exists())

    def test_unsafe_mirror_rejected_before_any_network_request(self):
        unsafe = [
            "http://mirror.example.test/",
            "https://user@mirror.example.test/",
            "https://mirror.example.test/%2e%2e/",
            "https://mirror.example.test/../",
            "https://mirror.example.test/?token=1",
            "https://mirror.example.test/#fragment",
            "https://mirror.example.test/\\evil/",
            "https://mirror.example.test:8443/",
            "https://mirror.example.test/not-a-directory",
        ]
        for base in unsafe:
            with self.subTest(base=base), self.assertRaisesRegex(ExportStagingError, "mirror base"):
                stage_export(
                    self.report, self.dest, self.output,
                    fetcher=lambda _url: self.fail("unexpected source lookup"),
                    opener=lambda *_args, **_kw: self.fail("unexpected download"),
                    mirror_base=base,
                )
        self.assertFalse(self.dest.exists())

    def test_mirror_cannot_bypass_required_official_sha256_preflight(self):
        forged = copy.deepcopy(self.report)
        forged["files"][0]["sha256"] = "f" * 64
        with self.assertRaisesRegex(ExportStagingError, "does not match"):
            stage_export(
                forged, self.dest, self.output, fetcher=self.fetch,
                opener=lambda *_args, **_kw: self.fail("download before preflight"),
                mirror_base="https://mirror.example.test/",
            )
        self.assertFalse(self.dest.exists())

    def test_bounded_retry_after_on_transient_rate_limit(self):
        attempts = []
        delays = []

        def throttled(request, timeout):
            attempts.append(request.full_url)
            if len(attempts) == 1:
                raise HTTPError(request.full_url, 429, "rate limit", {"Retry-After": "2"}, None)
            return self.opener(request, timeout)

        result = stage_export(
            self.report, self.dest, self.output, fetcher=self.fetch,
            opener=throttled, retry_sleep=delays.append,
        )
        self.assertEqual(len(result["files"]), 2)
        self.assertEqual(delays, [2])
        self.assertEqual(len(attempts), 3)

    def test_rate_limit_retry_is_bounded_and_never_publishes_partial_manifest(self):
        attempts = []
        delays = []

        def exhausted(request, timeout):
            attempts.append(request.full_url)
            raise HTTPError(request.full_url, 503, "unavailable", {}, None)

        with self.assertRaisesRegex(ExportStagingError, "retry budget exhausted"):
            stage_export(
                self.report, self.dest, self.output, fetcher=self.fetch,
                opener=exhausted, retry_sleep=delays.append,
            )
        self.assertEqual(len(attempts), 3)
        self.assertEqual(delays, [1, 2])
        self.assertFalse(self.output.exists())
        self.assertFalse((self.dest / NAMES[0]).exists())

        for header in ("999999", "-1", "bad-date"):
            with self.subTest(header=header):
                with self.assertRaisesRegex(ExportStagingError, "Retry-After"):
                    stage_export(
                        self.report, self.dest, self.output, fetcher=self.fetch,
                        opener=lambda req, timeout: (_ for _ in ()).throw(
                            HTTPError(req.full_url, 429, "limit", {"Retry-After": header}, None)
                        ),
                        retry_sleep=lambda _: self.fail("unsafe retry sleep"),
                    )
                self.assertFalse(self.output.exists())

    def test_corruption_does_not_publish_manifest(self):
        def bad(request, timeout):
            name = request.full_url.rsplit("/", 1)[-1]
            if name == NAMES[0]:
                return Response(b"x" * len(BODY_A), request.full_url,
                                headers={"Content-Length": str(len(BODY_A))})
            return self.opener(request, timeout)

        with self.assertRaisesRegex(ExportStagingError, "SHA-256 mismatch"):
            self.stage(opener=bad)
        self.assertFalse(self.output.exists())
        self.assertFalse((self.dest / NAMES[0]).exists())

    @unittest.skipUnless(hasattr(os, "O_NOFOLLOW"), "requires POSIX no-follow")
    def test_symlink_swap_at_hash_open_is_rejected(self):
        self.dest.mkdir()
        target = self.dest / NAMES[0]
        target.write_bytes(BODY_A)
        external = self.root / "external"
        external.write_bytes(BODY_A)
        true_open = os.open
        changed = False

        def replace_on_open(path, flags, *args, **kwargs):
            nonlocal changed
            if Path(path) == target and not changed:
                changed = True
                target.unlink()
                target.symlink_to(external)
            return true_open(path, flags, *args, **kwargs)

        with patch("stage_current_content_export.os.open", side_effect=replace_on_open):
            with self.assertRaisesRegex(ExportStagingError, "unsafe source member"):
                self.stage()
        self.assertTrue(changed)
        self.assertFalse(self.output.exists())
        self.assertEqual(external.read_bytes(), BODY_A)

    def test_hardlinked_member_does_not_get_verification_receipt(self):
        self.dest.mkdir()
        external = self.root / "original"
        external.write_bytes(BODY_A)
        os.link(external, self.dest / NAMES[0])
        with self.assertRaisesRegex(ExportStagingError, "hardlink"):
            self.stage()
        self.assertFalse(self.output.exists())

    def test_forged_inventory_rejected_before_download(self):
        bad = copy.deepcopy(self.report)
        bad["files"][0]["sha256"] = "f" * 64
        with self.assertRaisesRegex(ExportStagingError, "does not match"):
            self.stage(
                opener=lambda *_args, **_kwargs: self.fail("unexpected download"),
                report=bad
            )
        self.assertFalse(self.dest.exists())

    def test_no_overwriting_existing_members_or_manifest(self):
        self.dest.mkdir()
        (self.dest / NAMES[0]).write_bytes(b"unverified")
        with self.assertRaisesRegex(ExportStagingError, "existing member"):
            self.stage()
        self.assertFalse(self.output.exists())
        (self.dest / NAMES[0]).unlink()
        self.stage()
        with self.assertRaisesRegex(ExportStagingError, "already exists"):
            self.stage()


if __name__ == "__main__":
    unittest.main()
