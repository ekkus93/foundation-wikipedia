#!/usr/bin/env python3
"""Measure how closely locally rendered Parsoid HTML matches official output.

``summarize`` extracts structure (headings, tables, references, internal links,
error markers). ``compare`` turns two summaries into fidelity metrics. The
``report`` command renders a sample through the harness wiki and fetches the
same revisions from the live wiki; it records measurements and only fails when
a page could not be fetched at all.
"""
import argparse
import difflib
import html.parser
import json
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

import check_renderer_fixtures as crf

USER_AGENT = "foundation-wikipedia-renderer-harness (https://github.com/ekkus93/foundation-wikipedia)"
ERROR_MARKERS = ("Script error", "Lua error", "Template loop detected", 'class="error', "Lua timeout")
MAX_LIVE_BYTES = 16 * 1024 * 1024


class _Structure(html.parser.HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.headings = []
        self.tables = 0
        self.refs = 0
        self.links = set()
        self._heading = None

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag in ("h1", "h2", "h3", "h4", "h5", "h6"):
            self._heading = [int(tag[1]), []]
        elif tag == "table":
            self.tables += 1
        elif tag == "sup" and "mw:Extension/ref" in (attrs.get("typeof") or ""):
            self.refs += 1
        elif tag == "a":
            href = attrs.get("href") or ""
            if href.startswith("./") and "#" not in href and "rel" in attrs and "mw:WikiLink" in attrs["rel"]:
                self.links.add(urllib.parse.unquote(href[2:]))

    def handle_endtag(self, tag):
        if self._heading is not None and tag == f"h{self._heading[0]}":
            self.headings.append((self._heading[0], " ".join("".join(self._heading[1]).split())))
            self._heading = None

    def handle_data(self, data):
        if self._heading is not None:
            self._heading[1].append(data)


def summarize(markup):
    parser = _Structure()
    parser.feed(markup)
    parser.close()
    return {
        "headings": parser.headings,
        "tables": parser.tables,
        "refs": parser.refs,
        "links": sorted(parser.links),
        "error_markers": {m: markup.count(m) for m in ERROR_MARKERS if m in markup},
    }


def _jaccard(a, b):
    a, b = set(a), set(b)
    return 1.0 if not (a or b) else len(a & b) / len(a | b)


def compare(local, live):
    """Fidelity metrics for one page; 1.0 means structurally identical."""
    return {
        "heading_similarity": difflib.SequenceMatcher(None, local["headings"], live["headings"]).ratio(),
        "link_jaccard": _jaccard(local["links"], live["links"]),
        "table_delta": local["tables"] - live["tables"],
        "ref_delta": local["refs"] - live["refs"],
        "local_error_markers": sum(local["error_markers"].values()),
        "live_error_markers": sum(live["error_markers"].values()),
    }


def aggregate(rows):
    """Summary across pages; every figure is a measurement, not a verdict."""
    n = len(rows)
    if not n:
        return {"pages": 0}
    return {
        "pages": n,
        "mean_heading_similarity": sum(r["metrics"]["heading_similarity"] for r in rows) / n,
        "mean_link_jaccard": sum(r["metrics"]["link_jaccard"] for r in rows) / n,
        "pages_with_local_errors": sum(1 for r in rows if r["metrics"]["local_error_markers"] > 0),
        "pages_with_ref_mismatch": sum(1 for r in rows if r["metrics"]["ref_delta"] != 0),
        "pages_with_table_mismatch": sum(1 for r in rows if r["metrics"]["table_delta"] != 0),
    }


def fetch_live(host, revision_id, timeout=60):
    url = f"https://{host}/w/rest.php/v1/revision/{int(revision_id)}/with_html"
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=timeout) as response:
        raw = response.read(MAX_LIVE_BYTES + 1)
    if len(raw) > MAX_LIVE_BYTES:
        raise ValueError(f"live response for revision {revision_id} is too large")
    payload = json.loads(raw.decode("utf-8"))
    if not isinstance(payload.get("html"), str) or not payload["html"].strip():
        raise ValueError(f"live response for revision {revision_id} has no html")
    return payload["html"]


def build_report(sample, local_base, live_host, pause=1.0):
    rows, failures = [], []
    for entry in sample:
        row = {"title": entry["title"], "source_revision_id": entry["revision_id"]}
        try:
            started = time.monotonic()
            local = crf.fetch_page(local_base, entry["title"])
            row["local_render_seconds"] = round(time.monotonic() - started, 3)
            local_html = crf._html_of(local)
            if local["status"] != 200 or local_html is None:
                raise ValueError(f"local render status {local['status']}")
            live_html = fetch_live(live_host, entry["revision_id"])
            row["metrics"] = compare(summarize(local_html), summarize(live_html))
            row["local_html_bytes"] = len(local_html.encode("utf-8"))
            row["live_html_bytes"] = len(live_html.encode("utf-8"))
        except (OSError, ValueError, urllib.error.URLError) as error:
            failures.append(f"{entry['title']}: {error}")
            continue
        rows.append(row)
        time.sleep(pause)
    return {"schema": 1, "live_host": live_host, "aggregate": aggregate(rows), "pages": rows,
            "failures": failures}


def _main(argv):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sample", required=True)
    parser.add_argument("--local-base", required=True)
    parser.add_argument("--live-host", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args(argv)
    with open(args.sample, encoding="utf-8") as handle:
        sample = json.load(handle)["sample"]
    report = build_report(sample, args.local_base, args.live_host)
    with open(args.out, "w", encoding="utf-8") as handle:
        json.dump(report, handle, ensure_ascii=False, indent=2, sort_keys=True)
        handle.write("\n")
    print(json.dumps(report["aggregate"], sort_keys=True))
    for failure in report["failures"]:
        print("FAIL:", failure, file=sys.stderr)
    return 1 if report["failures"] else 0


if __name__ == "__main__":
    sys.exit(_main(sys.argv[1:]))
