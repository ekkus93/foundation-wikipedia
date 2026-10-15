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
import html
import html.parser
import re
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
        self.redlinks = set()
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
            if href.startswith("./") and "rel" in attrs and "mw:WikiLink" in attrs["rel"]:
                parts = urllib.parse.urlsplit(href)
                if parts.fragment:
                    return  # Section anchors are not page-level targets.
                redlink = sorted(urllib.parse.parse_qsl(parts.query)) == [
                    ("action", "edit"), ("redlink", "1")]
                if parts.query and not redlink:
                    return  # Do not erase arbitrary meaningful query strings.
                title = urllib.parse.unquote(parts.path[2:])
                if title:
                    self.links.add(title)
                    if redlink:
                        self.redlinks.add(title)

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
        "redlinks": sorted(parser.redlinks),
        "error_markers": {m: markup.count(m) for m in ERROR_MARKERS if m in markup},
    }


def _jaccard(a, b):
    a, b = set(a), set(b)
    return 1.0 if not (a or b) else len(a & b) / len(a | b)


def compare(local, live):
    """Fidelity metrics for one page; 1.0 means structurally identical.

    Link differences are samples, not a claim that the live revision's
    template/module dependency versions match the local dump's versions.
    """
    missing = sorted(set(live["links"]) - set(local["links"]))
    unexpected = sorted(set(local["links"]) - set(live["links"]))
    return {
        "heading_similarity": difflib.SequenceMatcher(None, local["headings"], live["headings"]).ratio(),
        "link_jaccard": _jaccard(local["links"], live["links"]),
        "redlink_status_drift_count": len(
            set(local.get("redlinks", [])) ^ set(live.get("redlinks", []))),
        "missing_link_count": len(missing),
        "unexpected_link_count": len(unexpected),
        "missing_links_sample": missing[:10],
        "unexpected_links_sample": unexpected[:10],
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
        "pages_below_half_link_overlap": sum(1 for r in rows if r["metrics"]["link_jaccard"] < 0.5),
        "pages_with_redlink_status_drift": sum(
            1 for r in rows if r["metrics"]["redlink_status_drift_count"] > 0),
        "local_error_page_samples": [
            {"title": r.get("title"), "source_revision_id": r.get("source_revision_id"),
             "markers": r.get("local_error_details", {})}
            for r in rows if r["metrics"]["local_error_markers"] > 0
        ][:10],
    }


def _error_excerpt(markup):
    """Bounded human-readable context; never dump full rendered HTML to CI."""
    hits = [markup.find(marker) for marker in ERROR_MARKERS]
    hits = [hit for hit in hits if hit >= 0]
    if not hits:
        return ""
    at = min(hits)
    fragment = markup[max(0, at - 30):at + 210]
    return " ".join(html.unescape(re.sub(r"<[^>]*>", " ", fragment)).split())[:180]


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
            local_summary, live_summary = summarize(local_html), summarize(live_html)
            row["metrics"] = compare(local_summary, live_summary)
            if local_summary["error_markers"]:
                row["local_error_details"] = local_summary["error_markers"]
                row["local_error_excerpt"] = _error_excerpt(local_html)
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
    # Keep the workflow's existing diagnostic-vs-acceptance distinction:
    # only missing fetches fail here. Print bounded per-page evidence so a
    # green import does not conceal missing templates/Lua or link drift.
    for row in report["pages"][:50]:
        metrics = row["metrics"]
        if metrics["local_error_markers"] or metrics["link_jaccard"] < 0.5:
            print("FIDELITY-WARNING:", json.dumps({
                "title": row["title"], "source_revision_id": row["source_revision_id"],
                "link_jaccard": metrics["link_jaccard"],
                "missing_link_count": metrics["missing_link_count"],
                "unexpected_link_count": metrics["unexpected_link_count"],
                "missing_links_sample": metrics["missing_links_sample"],
                "unexpected_links_sample": metrics["unexpected_links_sample"],
                "local_error_details": row.get("local_error_details", {}),
                "local_error_excerpt": row.get("local_error_excerpt", ""),
                "redlink_status_drift_count": metrics["redlink_status_drift_count"],
            }, ensure_ascii=False, sort_keys=True), file=sys.stderr)
    for failure in report["failures"]:
        print("FAIL:", failure, file=sys.stderr)
    return 1 if report["failures"] else 0


if __name__ == "__main__":
    sys.exit(_main(sys.argv[1:]))
