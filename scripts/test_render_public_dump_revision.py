"""Tests for exact-revision public-dump HTML enrichment."""
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from urllib.error import HTTPError

from render_public_dump_revision import (
    RenderError,
    fetch_revision_html,
    render_verified_ndjson,
    revision_with_html_url,
    wikipedia_origin,
)


class Headers(dict):
    pass


class FakeResponse:
    def __init__(self, payload, url, *, status=200, content_type="application/json"):
        self._raw = json.dumps(payload).encode("utf-8")
        self._url = url
        self.status = status
        self.headers = Headers({"Content-Type": content_type})

    def read(self, limit=-1):
        return self._raw if limit < 0 else self._raw[:limit]

    def geturl(self):
        return self._url

    def __enter__(self):
        return self

    def __exit__(self, *_args):
        return False


class FakeOpener:
    def __init__(self, responses):
        self.responses = list(responses)
        self.requests = []

    def open(self, request, timeout):
        self.requests.append((request, timeout))
        item = self.responses.pop(0)
        if isinstance(item, Exception):
            raise item
        return item


def raw_record():
    return {
        "project": "enwiki",
        "generation_id": "2026-10-01",
        "page_id": 42,
        "namespace": 0,
        "title": "Gravity",
        "revision_id": 764138197,
        "timestamp": "2017-02-07T00:00:00Z",
        "wikitext": "'''Gravity'''",
        "wikitext_sha256": hashlib.sha256(b"'''Gravity'''").hexdigest(),
        "redirect_title": None,
        "source_member": "enwiki-current.xml.bz2",
        "source_member_url": "https://dumps.wikimedia.org/example",
        "source_member_sha256": "a" * 64,
    }


class RevisionRendererTests(unittest.TestCase):
    def test_project_mapping_is_allowlisted_to_wikipedia_hosts(self):
        self.assertEqual(wikipedia_origin("enwiki"), "https://en.wikipedia.org")
        self.assertEqual(wikipedia_origin("simplewiki"), "https://simple.wikipedia.org")
        self.assertEqual(wikipedia_origin("frwiki"), "https://fr.wikipedia.org")
        for project in [
            "",
            "commonswiki",
            "pt-brwiki",
            "zh_classicalwiki",
            "../enwiki",
            "enwiktionary",
            "en_wiki",
        ]:
            with self.subTest(project=project):
                with self.assertRaises(RenderError):
                    wikipedia_origin(project)

    def test_exact_revision_response_is_verified(self):
        record = raw_record()
        url = revision_with_html_url(record["project"], record["revision_id"])
        payload = {
            "id": record["revision_id"],
            "timestamp": record["timestamp"],
            "html": "<html><body><p>Gravity</p></body></html>",
        }
        opener = FakeOpener([FakeResponse(payload, url)])
        result = fetch_revision_html(
            record["project"],
            record["revision_id"],
            record["timestamp"],
            opener=opener,
        )
        self.assertEqual(result["renderer_url"], url)
        self.assertEqual(result["renderer_revision_id"], record["revision_id"])
        self.assertEqual(
            result["html_sha256"],
            hashlib.sha256(payload["html"].encode()).hexdigest(),
        )
        request, timeout = opener.requests[0]
        self.assertEqual(request.full_url, url)
        self.assertEqual(timeout, 30)
        self.assertEqual(request.get_header("Accept"), "application/json")

    def test_revision_identity_redirect_and_content_type_fail_closed(self):
        record = raw_record()
        url = revision_with_html_url(record["project"], record["revision_id"])
        good = {
            "id": record["revision_id"],
            "timestamp": record["timestamp"],
            "html": "<p>ok</p>",
        }
        cases = [
            FakeResponse({**good, "id": record["revision_id"] + 1}, url),
            FakeResponse({**good, "timestamp": "2026-01-01T00:00:00Z"}, url),
            FakeResponse(good, "https://evil.example/redirect"),
            FakeResponse(good, url, content_type="text/html"),
        ]
        for response in cases:
            with self.subTest(response=response):
                with self.assertRaises(RenderError):
                    fetch_revision_html(
                        record["project"],
                        record["revision_id"],
                        record["timestamp"],
                        opener=FakeOpener([response]),
                    )

    def test_html_and_response_budgets_fail_closed(self):
        record = raw_record()
        url = revision_with_html_url(record["project"], record["revision_id"])
        payload = {
            "id": record["revision_id"],
            "timestamp": record["timestamp"],
            "html": "x" * 101,
        }
        with self.assertRaisesRegex(RenderError, "HTML exceeded"):
            fetch_revision_html(
                record["project"],
                record["revision_id"],
                record["timestamp"],
                opener=FakeOpener([FakeResponse(payload, url)]),
                max_html_bytes=100,
            )
        with self.assertRaisesRegex(RenderError, "invalid rendered HTML"):
            fetch_revision_html(
                record["project"],
                record["revision_id"],
                record["timestamp"],
                opener=FakeOpener([FakeResponse(payload, url)]),
                max_html_bytes=0,
            )

    def test_rate_limit_retry_is_bounded(self):
        record = raw_record()
        url = revision_with_html_url(record["project"], record["revision_id"])
        error = HTTPError(url, 429, "rate limited", Headers({"Retry-After": "2"}), None)
        payload = {
            "id": record["revision_id"],
            "timestamp": record["timestamp"],
            "html": "<p>ok</p>",
        }
        delays = []
        opener = FakeOpener([error, FakeResponse(payload, url)])
        result = fetch_revision_html(
            record["project"],
            record["revision_id"],
            record["timestamp"],
            opener=opener,
            sleep=delays.append,
        )
        self.assertEqual(result["html"], "<p>ok</p>")
        self.assertEqual(delays, [2])
        self.assertEqual(len(opener.requests), 2)

        excessive = HTTPError(url, 503, "busy", Headers({"Retry-After": "31"}), None)
        with self.assertRaisesRegex(RenderError, "exceeds policy"):
            fetch_revision_html(
                record["project"],
                record["revision_id"],
                record["timestamp"],
                opener=FakeOpener([excessive]),
                sleep=lambda _seconds: None,
            )

    def test_ndjson_enrichment_is_atomic_and_preserves_dump_provenance(self):
        record = raw_record()
        url = revision_with_html_url(record["project"], record["revision_id"])
        payload = {
            "id": record["revision_id"],
            "timestamp": record["timestamp"],
            "html": "<html><body>Rendered gravity</body></html>",
        }
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = root / "raw.ndjson"
            output = root / "rendered.ndjson"
            source.write_text(json.dumps(record) + "\n", encoding="utf-8")
            self.assertEqual(
                render_verified_ndjson(
                    source,
                    output,
                    opener=FakeOpener([FakeResponse(payload, url)]),
                ),
                1,
            )
            enriched = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(enriched["source_member_sha256"], "a" * 64)
            self.assertEqual(enriched["revision_id"], record["revision_id"])
            self.assertEqual(enriched["renderer_revision_id"], record["revision_id"])
            self.assertEqual(enriched["html"], payload["html"])
            with self.assertRaisesRegex(RenderError, "already exists"):
                render_verified_ndjson(
                    source,
                    output,
                    opener=FakeOpener([FakeResponse(payload, url)]),
                )

    def test_invalid_record_never_publishes_partial_output(self):
        record = raw_record()
        record["source_member_sha256"] = "bad"
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = root / "raw.ndjson"
            output = root / "rendered.ndjson"
            source.write_text(json.dumps(record) + "\n", encoding="utf-8")
            with self.assertRaisesRegex(RenderError, "source_member_sha256"):
                render_verified_ndjson(source, output, opener=FakeOpener([]))
            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
