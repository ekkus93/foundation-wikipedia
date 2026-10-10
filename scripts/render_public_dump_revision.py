#!/usr/bin/env python3
"""Attach official MediaWiki/Parsoid HTML to verified public-dump records.

Input is the raw revision-scoped NDJSON emitted by import_public_dump_xml.py.
For selected articles, this stage asks the official Wikipedia REST API for the
*same revision* with HTML and verifies the returned revision identity before
publishing enriched NDJSON. It does not claim that public REST rendering is a
full-snapshot bulk service.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import tempfile
import time
from urllib.error import HTTPError, URLError
from urllib.request import HTTPRedirectHandler, Request, build_opener

MAX_INPUT_LINE_BYTES = 24 * 1024 * 1024
DEFAULT_MAX_HTML_BYTES = 32 * 1024 * 1024
ABSOLUTE_MAX_HTML_BYTES = 64 * 1024 * 1024
DEFAULT_MAX_RECORDS = 100_000
MAX_RETRY_AFTER_SECONDS = 30
RETRYABLE_STATUS = {429, 503}
USER_AGENT = "FoundationWikipedia/0.1 (public-dump revision renderer)"


class RenderError(ValueError):
    pass


class _NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, msg, headers, newurl):
        return None


def wikipedia_origin(project):
    """Map supported Wikipedia database names to their canonical wiki origin."""
    if project == "simplewiki":
        return "https://simple.wikipedia.org"
    match = re.fullmatch(r"([a-z][a-z0-9-]{0,31})wiki", project or "")
    if match is None:
        raise RenderError("unsupported Wikipedia project for revision renderer")
    language = match.group(1)
    return f"https://{language}.wikipedia.org"


def revision_with_html_url(project, revision_id):
    if type(revision_id) is not int or revision_id <= 0:
        raise RenderError("invalid revision ID")
    return f"{wikipedia_origin(project)}/w/rest.php/v1/revision/{revision_id}/with_html"


def _retry_delay(error):
    value = error.headers.get("Retry-After") if error.headers is not None else None
    if value is None:
        return 1
    if not value.isascii() or not value.isdigit():
        raise RenderError("invalid Retry-After from revision renderer") from error
    delay = int(value)
    if delay > MAX_RETRY_AFTER_SECONDS:
        raise RenderError("revision renderer Retry-After exceeds policy") from error
    return delay


def fetch_revision_html(project, revision_id, expected_timestamp=None, *,
                        opener=None, timeout=30, max_html_bytes=DEFAULT_MAX_HTML_BYTES,
                        sleep=time.sleep):
    """Fetch and verify exact-revision HTML from the official wiki REST API."""
    if type(timeout) not in (int, float) or timeout <= 0:
        raise RenderError("invalid renderer timeout")
    if (type(max_html_bytes) is not int or max_html_bytes <= 0
            or max_html_bytes > ABSOLUTE_MAX_HTML_BYTES):
        raise RenderError("invalid rendered HTML byte budget")
    if expected_timestamp is not None and (
            not isinstance(expected_timestamp, str) or not expected_timestamp):
        raise RenderError("invalid expected revision timestamp")

    url = revision_with_html_url(project, revision_id)
    opener = opener or build_opener(_NoRedirect())
    request = Request(
        url,
        headers={
            "Accept": "application/json",
            "User-Agent": USER_AGENT,
        },
        method="GET",
    )

    response = None
    for attempt in range(3):
        try:
            response = opener.open(request, timeout=timeout)
            break
        except HTTPError as error:
            if error.code not in RETRYABLE_STATUS or attempt == 2:
                raise RenderError(f"revision renderer HTTP {error.code}") from error
            sleep(_retry_delay(error))
        except (URLError, TimeoutError, OSError) as error:
            raise RenderError("revision renderer transport failure") from error
    if response is None:
        raise RenderError("revision renderer retry budget exhausted")

    with response:
        status = getattr(response, "status", 200)
        if status != 200:
            raise RenderError(f"revision renderer HTTP {status}")
        final_url = response.geturl()
        if final_url != url:
            raise RenderError("revision renderer redirected away from exact endpoint")
        content_type = response.headers.get("Content-Type", "")
        if content_type.split(";", 1)[0].strip().lower() != "application/json":
            raise RenderError("revision renderer returned unexpected content type")
        # JSON escaping can exceed the final HTML byte count substantially.
        raw = response.read(ABSOLUTE_MAX_HTML_BYTES * 2 + 1)
    if len(raw) > ABSOLUTE_MAX_HTML_BYTES * 2:
        raise RenderError("revision renderer response exceeded byte budget")
    try:
        payload = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise RenderError("invalid revision renderer JSON") from error
    if not isinstance(payload, dict):
        raise RenderError("invalid revision renderer payload")
    if payload.get("id") != revision_id:
        raise RenderError("revision renderer returned mismatched revision ID")
    timestamp = payload.get("timestamp")
    if expected_timestamp is not None and timestamp != expected_timestamp:
        raise RenderError("revision renderer returned mismatched revision timestamp")
    html = payload.get("html")
    if not isinstance(html, str) or not html.strip():
        raise RenderError("revision renderer returned empty HTML")
    html_bytes = html.encode("utf-8")
    if len(html_bytes) > max_html_bytes:
        raise RenderError("rendered HTML exceeded byte budget")
    return {
        "html": html,
        "html_sha256": hashlib.sha256(html_bytes).hexdigest(),
        "renderer_url": url,
        "renderer_revision_id": revision_id,
        "renderer_timestamp": timestamp,
        "renderer_kind": "wikimedia-rest-revision-with-html",
    }


def _validate_raw_record(record):
    if not isinstance(record, dict):
        raise RenderError("raw dump record must be an object")
    project = record.get("project")
    revision_id = record.get("revision_id")
    timestamp = record.get("timestamp")
    page_id = record.get("page_id")
    if not isinstance(project, str) or not project:
        raise RenderError("raw dump record missing project")
    if type(revision_id) is not int or revision_id <= 0:
        raise RenderError("raw dump record has invalid revision ID")
    if not isinstance(timestamp, str) or not timestamp:
        raise RenderError("raw dump record missing revision timestamp")
    if type(page_id) is not int or page_id <= 0:
        raise RenderError("raw dump record has invalid page ID")
    for field in ("wikitext_sha256", "source_member_sha256"):
        digest = record.get(field)
        if (not isinstance(digest, str) or len(digest) != 64
                or any(c not in "0123456789abcdefABCDEF" for c in digest)):
            raise RenderError(f"raw dump record has invalid {field}")
    wikipedia_origin(project)
    return project, revision_id, timestamp


def render_verified_ndjson(raw_input, output, *, opener=None, timeout=30,
                           max_html_bytes=DEFAULT_MAX_HTML_BYTES,
                           max_records=DEFAULT_MAX_RECORDS, sleep=time.sleep):
    """Atomically enrich bounded raw dump NDJSON with exact-revision HTML."""
    if type(max_records) is not int or max_records <= 0:
        raise RenderError("invalid renderer record budget")
    raw_input = Path(raw_input)
    output = Path(output)
    if output.parent.is_symlink() or not output.parent.is_dir():
        raise RenderError("unsafe renderer output directory")
    temporary = None
    count = 0
    try:
        with raw_input.open("rb") as source, tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            dir=output.parent,
            prefix=".rendered-dump.",
            suffix=".tmp",
            delete=False,
        ) as writer:
            temporary = Path(writer.name)
            for line_number, line in enumerate(source, 1):
                if len(line) > MAX_INPUT_LINE_BYTES:
                    raise RenderError(f"raw dump line {line_number} exceeded byte budget")
                if not line.strip():
                    raise RenderError(f"raw dump line {line_number} is empty")
                try:
                    record = json.loads(line.decode("utf-8"))
                except (UnicodeDecodeError, json.JSONDecodeError) as error:
                    raise RenderError(f"invalid raw dump JSON at line {line_number}") from error
                project, revision_id, timestamp = _validate_raw_record(record)
                count += 1
                if count > max_records:
                    raise RenderError("renderer record budget exceeded")
                rendered = fetch_revision_html(
                    project,
                    revision_id,
                    timestamp,
                    opener=opener,
                    timeout=timeout,
                    max_html_bytes=max_html_bytes,
                    sleep=sleep,
                )
                enriched = dict(record)
                enriched.update(rendered)
                writer.write(json.dumps(enriched, ensure_ascii=False, sort_keys=True) + "\n")
            if count == 0:
                raise RenderError("raw dump render input is empty")
            writer.flush()
            os.fsync(writer.fileno())
        try:
            os.link(temporary, output)
        except FileExistsError as error:
            raise RenderError("rendered dump output already exists") from error
        directory_fd = os.open(output.parent, os.O_RDONLY)
        try:
            os.fsync(directory_fd)
        finally:
            os.close(directory_fd)
        return count
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("raw_ndjson", type=Path)
    parser.add_argument("rendered_ndjson", type=Path)
    parser.add_argument("--timeout", type=float, default=30)
    parser.add_argument("--max-html-bytes", type=int, default=DEFAULT_MAX_HTML_BYTES)
    parser.add_argument("--max-records", type=int, default=DEFAULT_MAX_RECORDS)
    args = parser.parse_args()
    try:
        count = render_verified_ndjson(
            args.raw_ndjson,
            args.rendered_ndjson,
            timeout=args.timeout,
            max_html_bytes=args.max_html_bytes,
            max_records=args.max_records,
        )
    except (RenderError, OSError, ValueError) as error:
        parser.exit(2, f"Public-dump revision rendering failed: {error}\n")
    print(
        f"Rendered {count} exact public-dump revision(s) through official MediaWiki REST. "
        "This is a selected-article fallback, not a bulk full-snapshot rendering service."
    )


if __name__ == "__main__":
    main()
