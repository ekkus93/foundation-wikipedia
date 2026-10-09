"""Offline regression coverage for strict resumable official-dump transport."""
import hashlib
from pathlib import Path
import tempfile
import unittest

from download_public_dump import DownloadError, fetch_dump_member

BODY = b"abcde"
URL = "https://dumps.wikimedia.org/enwiki/20261009/articles.xml"
AGENT = "FoundationWikipedia/0.1 (https://github.com/ekkus93/foundation-wikipedia)"


class Response:
    def __init__(self, status, body, headers=None, url=URL, fail_after=False):
        self.status = status
        self.body = body
        self.headers = headers or {}
        self.url = url
        self.fail_after = fail_after
        self.used = False

    def geturl(self):
        return self.url

    def read(self, _):
        if not self.used:
            self.used = True
            data = self.body
            self.body = b""
            return data
        if self.fail_after:
            raise OSError("simulated interrupted transfer")
        return b""

    def __enter__(self):
        return self

    def __exit__(self, *_):
        return False


class Opener:
    def __init__(self, responses):
        self.responses = iter(responses)
        self.requests = []

    def open(self, request, timeout):
        self.requests.append(request)
        assert timeout == 30
        return next(self.responses)


class DownloadTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name) / "staging"
        self.report = {
            "project": "enwiki",
            "generation_id": "20261009",
            "completed": True,
            "files": [{
                "name": "articles.xml",
                "url": URL,
                "bytes": len(BODY),
                "sha1": hashlib.sha1(BODY).hexdigest(),
            }],
        }

    def fetch(self, opener):
        return fetch_dump_member(self.report, "articles.xml", self.root, AGENT, opener)

    def test_complete_transfer_promotes_only_verified_bytes(self):
        opener = Opener([Response(200, BODY, {"Content-Length": "5"})])
        receipt = self.fetch(opener)
        self.assertEqual(receipt["sha256"], hashlib.sha256(BODY).hexdigest())
        self.assertEqual((self.root / "articles.xml").read_bytes(), BODY)
        self.assertFalse((self.root / "articles.xml.part").exists())
        self.assertIsNone(opener.requests[0].get_header("Range"))

    def test_interrupted_partial_resumes_with_exact_range(self):
        opener = Opener([Response(200, BODY[:3], fail_after=True)])
        with self.assertRaises(OSError):
            self.fetch(opener)
        self.assertEqual((self.root / "articles.xml.part").read_bytes(), b"abc")
        retry = Opener([Response(206, BODY[3:], {
            "Content-Length": "2", "Content-Range": "bytes 3-4/5"
        })])
        self.fetch(retry)
        self.assertEqual(retry.requests[0].get_header("Range"), "bytes=3-")
        self.assertEqual((self.root / "articles.xml").read_bytes(), BODY)

    def test_empty_partial_file_restarts_without_false_file_exists_error(self):
        self.root.mkdir()
        (self.root / "articles.xml.part").touch()
        receipt = self.fetch(Opener([Response(200, BODY)]))
        self.assertEqual(receipt["bytes"], len(BODY))
        self.assertEqual((self.root / "articles.xml").read_bytes(), BODY)

    def test_wrong_range_or_server_ignores_resume_fails_without_promotion(self):
        self.root.mkdir()
        (self.root / "articles.xml.part").write_bytes(BODY[:3])
        for response in [
            Response(200, BODY),
            Response(206, BODY[3:], {"Content-Range": "bytes 2-4/5"}),
        ]:
            with self.assertRaises(DownloadError):
                self.fetch(Opener([response]))
            self.assertFalse((self.root / "articles.xml").exists())

    def test_upstream_digest_failure_is_not_promoted(self):
        with self.assertRaisesRegex(DownloadError, "SHA-1 mismatch"):
            self.fetch(Opener([Response(200, b"zzzzz")]))
        self.assertFalse((self.root / "articles.xml").exists())

    def test_off_host_redirect_and_bad_source_identity_are_rejected(self):
        with self.assertRaisesRegex(DownloadError, "authorized URL"):
            self.fetch(Opener([Response(200, BODY, url="https://attacker.example/")]))
        self.report["files"][0]["url"] = "https://attacker.example/articles.xml"
        with self.assertRaisesRegex(DownloadError, "official URL"):
            self.fetch(Opener([]))

    def test_empty_or_oversized_metadata_and_symlinks_fail(self):
        self.report["files"][0]["bytes"] = True
        with self.assertRaises(DownloadError):
            self.fetch(Opener([]))
        self.report["files"][0]["bytes"] = 5
        outside = Path(self.root.parent) / "outside"
        outside.write_bytes(BODY)
        self.root.mkdir()
        (self.root / "articles.xml.part").symlink_to(outside)
        with self.assertRaisesRegex(DownloadError, "unsafe existing"):
            self.fetch(Opener([]))


if __name__ == "__main__":
    unittest.main()
