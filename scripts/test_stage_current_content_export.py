"""Offline transport regressions for a complete current-content export."""
import copy
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest

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
