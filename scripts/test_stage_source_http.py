"""Deterministic HTTP Range tests with no external network."""
import copy
import hashlib
import io
import tempfile
import unittest
from pathlib import Path

from stage_source_http import SourceDownloadError, stage_generation


class Response(io.BytesIO):
    def __init__(self, data, url, status=200, headers=None):
        super().__init__(data)
        self.url, self.status, self.headers = url, status, headers or {}

    def geturl(self):
        return self.url


class SourceDownloadTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.directory = Path(self.tmp.name)
        self.payload = b"revision bytes" * 20
        self.url = "https://dumps.wikimedia.org/enwiki/20261009/articles.xml"
        self.manifest = {
            "project": "enwiki", "generation_id": "20261009",
            "completed": True,
            "source_url": "https://dumps.wikimedia.org/enwiki/20261009/",
            "files": [{"name": "articles.xml", "bytes": len(self.payload),
                       "sha256": hashlib.sha256(self.payload).hexdigest()}],
        }
        self.requests = []

    def opener(self, request, timeout):
        start = request.get_header("Range")
        self.requests.append(start)
        if start:
            offset = int(start.split("=")[1].split("-")[0])
            return Response(
                self.payload[offset:], self.url, 206,
                {"Content-Range": f"bytes {offset}-{len(self.payload)-1}/{len(self.payload)}",
                 "Content-Length": str(len(self.payload) - offset)},
            )
        return Response(self.payload, self.url,
                        headers={"Content-Length": str(len(self.payload))})

    def stage(self, opener=None):
        return stage_generation(self.manifest, self.directory, opener=opener or self.opener)

    def test_fresh_download_and_skip_existing_verified(self):
        self.assertEqual(self.stage(), 1)
        self.assertEqual((self.directory / "articles.xml").read_bytes(), self.payload)
        self.assertEqual(self.stage(), 1)
        self.assertEqual(self.requests, [None])

    def test_exact_range_resume(self):
        (self.directory / "articles.xml.part").write_bytes(self.payload[:11])
        self.assertEqual(self.stage(), 1)
        self.assertEqual(self.requests, ["bytes=11-"])
        self.assertFalse((self.directory / "articles.xml.part").exists())

    def test_interruption_then_resume(self):
        def interrupted(request, timeout):
            return Response(self.payload[:9], self.url)
        with self.assertRaisesRegex(SourceDownloadError, "incomplete transfer"):
            self.stage(interrupted)
        self.assertEqual((self.directory / "articles.xml.part").read_bytes(), self.payload[:9])
        self.assertEqual(self.stage(), 1)

    def test_bad_hash_fails_closed(self):
        def corrupt(request, timeout):
            return Response(b"x" * len(self.payload), self.url)
        with self.assertRaisesRegex(SourceDownloadError, "checksum mismatch"):
            self.stage(corrupt)
        self.assertFalse((self.directory / "articles.xml").exists())
        self.assertFalse((self.directory / "articles.xml.part").exists())

    def test_bad_range_and_redirect(self):
        (self.directory / "articles.xml.part").write_bytes(self.payload[:5])
        def ignores(request, timeout):
            return Response(self.payload, self.url)
        with self.assertRaisesRegex(SourceDownloadError, "resume range"):
            self.stage(ignores)
        def wrong(request, timeout):
            return Response(self.payload[5:], self.url, 206,
                            {"Content-Range": f"bytes 4-{len(self.payload)-1}/{len(self.payload)}"})
        with self.assertRaisesRegex(SourceDownloadError, "Content-Range"):
            self.stage(wrong)
        def redirect(request, timeout):
            return Response(self.payload[5:], "https://attacker.example/file", 206)
        with self.assertRaisesRegex(SourceDownloadError, "untrusted origin"):
            self.stage(redirect)
        self.assertEqual((self.directory / "articles.xml.part").read_bytes(), self.payload[:5])

    def test_excess_bytes_and_bad_headers(self):
        def excess(request, timeout):
            return Response(self.payload + b"extra", self.url)
        with self.assertRaisesRegex(SourceDownloadError, "excess bytes"):
            self.stage(excess)
        (self.directory / "articles.xml.part").unlink()
        for headers in [{"Content-Length": "nope"}, {"Content-Length": "4"},
                        {"Content-Encoding": "gzip"}]:
            def invalid(request, timeout):
                return Response(self.payload, self.url, headers=headers)
            with self.assertRaises(SourceDownloadError):
                self.stage(invalid)
        self.assertFalse((self.directory / "articles.xml").exists())

    def test_untrusted_metadata_rejected_before_network(self):
        def forbidden(request, timeout):
            self.fail("network access before validation")
        for url in ["http://dumps.wikimedia.org/enwiki/",
                    "https://evil.example/enwiki/",
                    "https://dumps.wikimedia.org/enwiki/?secret=x"]:
            candidate = copy.deepcopy(self.manifest)
            candidate["source_url"] = url
            with self.assertRaises(SourceDownloadError):
                stage_generation(candidate, self.directory, opener=forbidden)
        self.manifest["files"][0]["name"] = "../escape"
        with self.assertRaises(SourceDownloadError):
            self.stage(forbidden)

    def test_symlinked_partial_and_existing_mismatch(self):
        outside = self.directory / "outside"
        outside.write_bytes(b"preserve")
        part = self.directory / "articles.xml.part"
        part.symlink_to(outside)
        with self.assertRaisesRegex(SourceDownloadError, "symlink"):
            self.stage()
        self.assertEqual(outside.read_bytes(), b"preserve")
        part.unlink()
        (self.directory / "articles.xml").write_bytes(b"wrong")
        with self.assertRaisesRegex(SourceDownloadError, "existing staged file differs"):
            self.stage()
        self.assertEqual(self.requests, [])


if __name__ == "__main__":
    unittest.main()
