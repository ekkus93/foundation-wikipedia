#!/usr/bin/env python3
"""Fetch and check fixture pages rendered by the pinned MediaWiki/Parsoid harness.

``fetch`` renders every manifest page twice through the REST ``with_html``
endpoint without following redirects. ``check`` applies the manifest
expectations and fails closed, so a missing page, unexpanded markup or a
non-repeatable render never counts as success.
"""
import argparse
import hashlib
import html.parser
import json
import sys
import urllib.error
import urllib.parse
import urllib.request

SCHEMA = 1
MAX_BODY_BYTES = 16 * 1024 * 1024
# Strings that mean a template, Lua module or parser function was not expanded.
FORBIDDEN_DEFAULT = (
    "{{",
    "Script error",
    "Lua error",
    "Template loop detected",
    "Template:Greet</a>",
)


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


def _positive_int(value):
    return type(value) is int and value > 0


def fetch_page(base_url, title, timeout=60):
    """Return status, Location header and decoded JSON payload for one title."""
    path = urllib.parse.quote(title.replace(" ", "_"), safe="")
    url = f"{base_url.rstrip('/')}/rest.php/v1/page/{path}/with_html"
    opener = urllib.request.build_opener(_NoRedirect)
    request = urllib.request.Request(url, headers={"User-Agent": "foundation-wikipedia-renderer-harness"})
    location = None
    try:
        with opener.open(request, timeout=timeout) as response:
            status, raw = response.status, response.read(MAX_BODY_BYTES + 1)
    except urllib.error.HTTPError as error:
        status, raw = error.code, error.read(MAX_BODY_BYTES + 1)
        location = error.headers.get("Location")
    if len(raw) > MAX_BODY_BYTES:
        raise ValueError(f"{title}: response exceeds {MAX_BODY_BYTES} bytes")
    try:
        payload = json.loads(raw.decode("utf-8"))
    except ValueError:
        payload = None
    return {"title": title, "status": status, "location": location, "payload": payload}


def load_manifest(path):
    with open(path, encoding="utf-8") as handle:
        manifest = json.load(handle)
    if manifest.get("schema") != SCHEMA:
        raise ValueError("unsupported renderer manifest schema")
    titles = [page["title"] for page in manifest["pages"]]
    if len(set(titles)) != len(titles):
        raise ValueError("duplicate page title in manifest")
    for entry in manifest["render"]:
        if entry["title"] not in titles:
            raise ValueError(f"render entry for unknown page: {entry['title']}")
    return manifest


class _TextExtractor(html.parser.HTMLParser):
    """Collect rendered text; attributes (including data-mw) are never text."""

    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.parts = []
        self._skip = 0

    def handle_starttag(self, tag, attrs):
        if tag in ("script", "style"):
            self._skip += 1

    def handle_endtag(self, tag):
        if tag in ("script", "style") and self._skip:
            self._skip -= 1

    def handle_data(self, data):
        if not self._skip:
            self.parts.append(data)


def visible_text(markup):
    """Whitespace-normalized text content, so inline spans don't split phrases."""
    extractor = _TextExtractor()
    extractor.feed(markup)
    extractor.close()
    return " ".join("".join(extractor.parts).split())


def _html_of(result):
    payload = result.get("payload")
    html = payload.get("html") if isinstance(payload, dict) else None
    return html if isinstance(html, str) else None


def check_page(result, expect):
    """Return a list of failure strings for one fetched page (empty means pass)."""
    title = result["title"]
    failures = []
    status = result.get("status")
    html = _html_of(result)
    target = expect.get("redirect_to")
    if target is not None:
        location = urllib.parse.unquote(result.get("location") or "")
        redirected = status in (301, 302, 303, 307, 308) and target in location
        in_body = status == 200 and html is not None and "mw:PageProp/redirect" in html and target in html
        if not (redirected or in_body):
            failures.append(f"{title}: expected redirect to {target}, got status {status}")
        return failures
    if status != 200:
        return [f"{title}: unexpected HTTP status {status}"]
    payload = result["payload"]
    latest = payload.get("latest") if isinstance(payload, dict) else None
    if not _positive_int(payload.get("id") if isinstance(payload, dict) else None):
        failures.append(f"{title}: missing or invalid page id")
    if not isinstance(latest, dict) or not _positive_int(latest.get("id")) or not isinstance(latest.get("timestamp"), str):
        failures.append(f"{title}: missing or invalid latest revision identity")
    if html is None or not html.strip():
        return failures + [f"{title}: empty rendered HTML"]
    for needle in expect.get("contains", []):
        if needle not in html:
            failures.append(f"{title}: missing expected markup {needle!r}")
    text = visible_text(html)
    for needle in expect.get("text_contains", []):
        if needle not in text:
            failures.append(f"{title}: missing expected content {needle!r}")
    for needle in FORBIDDEN_DEFAULT + tuple(expect.get("not_contains", [])):
        if needle in html:
            failures.append(f"{title}: forbidden content present {needle!r}")
    return failures


def check_results(manifest, passes):
    """Check every manifest render entry across all fetch passes."""
    failures = []
    if len(passes) < 2:
        failures.append("at least two render passes are required to check repeatability")
    by_pass = [{item["title"]: item for item in one} for one in passes]
    for entry in manifest["render"]:
        title = entry["title"]
        digests = set()
        for index, results in enumerate(by_pass):
            if title not in results:
                failures.append(f"{title}: missing from render pass {index}")
                continue
            failures.extend(check_page(results[title], entry.get("expect", {})))
            html = _html_of(results[title])
            if html is not None:
                digests.add(hashlib.sha256(html.encode("utf-8")).hexdigest())
        if len(digests) > 1:
            failures.append(f"{title}: render is not repeatable across passes")
    return failures


def build_evidence(manifest, passes):
    """Deterministic evidence record; observations never affect pass/fail."""
    first = {item["title"]: item for item in passes[0]}
    pages = []
    for entry in manifest["render"]:
        result = first.get(entry["title"], {})
        html = _html_of(result)
        payload = result.get("payload") if isinstance(result.get("payload"), dict) else {}
        latest = payload.get("latest") if isinstance(payload.get("latest"), dict) else {}
        pages.append({
            "title": entry["title"],
            "status": result.get("status"),
            "page_id": payload.get("id"),
            "revision_id": latest.get("id"),
            "revision_timestamp": latest.get("timestamp"),
            "html_sha256": hashlib.sha256(html.encode("utf-8")).hexdigest() if html else None,
            "html_bytes": len(html.encode("utf-8")) if html else 0,
            "observed": {marker: bool(html and marker in html) for marker in entry.get("observe", [])},
        })
    return {"schema": SCHEMA, "passes": len(passes), "pages": pages}


def _main(argv):
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    fetch = sub.add_parser("fetch")
    fetch.add_argument("--base-url", required=True)
    fetch.add_argument("--manifest", required=True)
    fetch.add_argument("--out", required=True)
    fetch.add_argument("--passes", type=int, default=2)
    check = sub.add_parser("check")
    check.add_argument("--manifest", required=True)
    check.add_argument("--results", required=True)
    check.add_argument("--evidence-out")
    args = parser.parse_args(argv)
    manifest = load_manifest(args.manifest)
    if args.command == "fetch":
        titles = [entry["title"] for entry in manifest["render"]]
        passes = [[fetch_page(args.base_url, title) for title in titles] for _ in range(args.passes)]
        with open(args.out, "w", encoding="utf-8") as handle:
            json.dump(passes, handle, ensure_ascii=False, sort_keys=True)
        return 0
    with open(args.results, encoding="utf-8") as handle:
        passes = json.load(handle)
    failures = check_results(manifest, passes)
    if args.evidence_out:
        with open(args.evidence_out, "w", encoding="utf-8") as handle:
            json.dump(build_evidence(manifest, passes), handle, ensure_ascii=False, indent=2, sort_keys=True)
            handle.write("\n")
    for failure in failures:
        print("FAIL:", failure, file=sys.stderr)
    print("renderer fixtures:", "FAILED" if failures else "ok")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(_main(sys.argv[1:]))
