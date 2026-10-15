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
import hashlib
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
MAX_LOCAL_RAW_BYTES = 64 * 1024 * 1024


class _Structure(html.parser.HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.headings = []
        self.tables = 0
        self.refs = 0
        self.links = set()
        self.redlinks = set()
        self._heading = None
        # These are reference counts, NOT downloaded/verified offline assets.
        self.images = 0
        self.images_without_src = 0
        self.images_with_srcset = 0
        self.remote_image_refs = 0
        self.mathml = 0
        self.svg = 0
        self.captions = 0
        self.video = 0
        self.audio = 0

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag in ("h1", "h2", "h3", "h4", "h5", "h6"):
            self._heading = [int(tag[1]), []]
        elif tag == "table":
            self.tables += 1
        elif tag == "img":
            self.images += 1
            source = attrs.get("src") or ""
            if not source:
                self.images_without_src += 1
            elif source.startswith(("https://", "http://", "//")):
                self.remote_image_refs += 1
            if attrs.get("srcset"):
                self.images_with_srcset += 1
        elif tag == "math":
            self.mathml += 1
        elif tag == "svg":
            self.svg += 1
        elif tag == "figcaption":
            self.captions += 1
        elif tag == "video":
            self.video += 1
        elif tag == "audio":
            self.audio += 1
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
        "visuals": {
            "images": parser.images,
            "images_without_src": parser.images_without_src,
            "images_with_srcset": parser.images_with_srcset,
            "remote_image_refs": parser.remote_image_refs,
            "mathml": parser.mathml,
            "inline_svg": parser.svg,
            "captions": parser.captions,
            "video": parser.video,
            "audio": parser.audio,
        },
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
        "image_count_delta": (local.get("visuals", {}).get("images", 0)
                              - live.get("visuals", {}).get("images", 0)),
        "mathml_count_delta": (local.get("visuals", {}).get("mathml", 0)
                               - live.get("visuals", {}).get("mathml", 0)),
        "local_visuals": local.get("visuals", {}),
        "live_visuals": live.get("visuals", {}),
        "local_error_markers": sum(local["error_markers"].values()),
        "live_error_markers": sum(live["error_markers"].values()),
        "error_marker_delta": {
            marker: local["error_markers"].get(marker, 0) - live["error_markers"].get(marker, 0)
            for marker in ERROR_MARKERS
            if local["error_markers"].get(marker, 0) != live["error_markers"].get(marker, 0)
        },
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
        "pages_with_live_errors": sum(1 for r in rows if r["metrics"]["live_error_markers"] > 0),
        "pages_with_local_only_errors": sum(
            1 for r in rows if r["metrics"]["local_error_markers"] > 0
            and r["metrics"]["live_error_markers"] == 0),
        "pages_with_error_marker_drift": sum(
            1 for r in rows if r["metrics"]["error_marker_delta"]),
        # XML import can allocate new local IDs without source corruption.
        "pages_with_local_page_id_difference": sum(
            1 for r in rows if r.get("local_id_comparison", {}).get("page_id_equal") is False),
        "pages_with_local_revision_id_difference": sum(
            1 for r in rows if r.get("local_id_comparison", {}).get("revision_id_equal") is False),
        "pages_with_source_timestamp_mismatch": sum(
            1 for r in rows if r.get("source_timestamp_match") is False),
        "pages_with_source_wikitext_mismatch": sum(
            1 for r in rows if r.get("source_wikitext_match") is False),
        "pages_without_source_wikitext_digest": sum(
            1 for r in rows if r.get("source_wikitext_match") is None),
        "pages_with_ref_mismatch": sum(1 for r in rows if r["metrics"]["ref_delta"] != 0),
        "pages_with_table_mismatch": sum(1 for r in rows if r["metrics"]["table_delta"] != 0),
        "pages_with_image_count_mismatch": sum(
            1 for r in rows if r["metrics"]["image_count_delta"] != 0),
        "pages_with_mathml_count_mismatch": sum(
            1 for r in rows if r["metrics"]["mathml_count_delta"] != 0),
        "pages_with_remote_image_references": sum(
            1 for r in rows if r["metrics"]["local_visuals"].get("remote_image_refs", 0) > 0),
        "total_local_image_references": sum(
            r["metrics"]["local_visuals"].get("images", 0) for r in rows),
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



def fetch_local_wikitext_sha256(base_url, title, local_page_id, local_revision_id, timeout=30):
    """Hash exact local main-slot raw text; reject ambiguous API identities.

    Do not emit raw article text in the report or CI logs. This measures
    whether the remapped local revision preserves its original source bytes.
    """
    query = urllib.parse.urlencode({
        "action": "query", "prop": "revisions", "rvprop": "ids|content",
        "rvslots": "main", "rvlimit": "1", "titles": title,
        "format": "json", "formatversion": "2",
    })
    request = urllib.request.Request(
        f"{base_url.rstrip('/')}/api.php?{query}", headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=timeout) as response:
        raw = response.read(MAX_LOCAL_RAW_BYTES + 1)
    if len(raw) > MAX_LOCAL_RAW_BYTES:
        raise ValueError("local raw revision response exceeds byte cap")
    payload = json.loads(raw.decode("utf-8"))
    if not isinstance(payload, dict) or payload.get("error"):
        raise ValueError("invalid local raw revision response")
    query_result = payload.get("query")
    pages = query_result.get("pages") if isinstance(query_result, dict) else None
    if not isinstance(pages, list) or len(pages) != 1:
        raise ValueError("ambiguous local raw page response")
    page = pages[0]
    if not isinstance(page, dict) or page.get("pageid") != local_page_id:
        raise ValueError("local raw page identity mismatch")
    revisions = page.get("revisions")
    if not isinstance(revisions, list) or len(revisions) != 1:
        raise ValueError("ambiguous local raw revision")
    revision = revisions[0]
    if not isinstance(revision, dict) or revision.get("revid") != local_revision_id:
        raise ValueError("local raw revision identity mismatch")
    slots = revision.get("slots")
    main = slots.get("main") if isinstance(slots, dict) else None
    if not isinstance(main, dict) or main.get("contentmodel") not in (None, "wikitext"):
        raise ValueError("local revision is not main-slot wikitext")
    wikitext = main.get("content")
    if not isinstance(wikitext, str):
        raise ValueError("missing local main-slot wikitext")
    return hashlib.sha256(wikitext.encode("utf-8")).hexdigest()


def build_report(sample, local_base, live_host, pause=1.0):
    rows, failures = [], []
    for entry in sample:
        row = {"title": entry["title"], "source_page_id": entry["page_id"],
               "source_revision_id": entry["revision_id"]}
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
            # HTML structure does not prove exact source page/revision identity.
            payload = local.get("payload") if isinstance(local.get("payload"), dict) else {}
            latest = payload.get("latest") if isinstance(payload.get("latest"), dict) else {}
            local_identity = {
                "page_id": payload.get("id"),
                "revision_id": latest.get("id"),
                "timestamp": latest.get("timestamp"),
            }
            row["local_identity"] = local_identity
            # Numeric local import IDs are not stable upstream identifiers.
            row["local_id_comparison"] = {
                "page_id_equal": local_identity["page_id"] == entry["page_id"],
                "revision_id_equal": local_identity["revision_id"] == entry["revision_id"],
            }
            row["source_timestamp_match"] = local_identity["timestamp"] == entry["timestamp"]
            source_digest = entry.get("source_wikitext_sha256")
            if source_digest is not None:
                if (not isinstance(source_digest, str) or len(source_digest) != 64
                        or any(c not in "0123456789abcdef" for c in source_digest)):
                    raise ValueError("invalid original source wikitext digest")
                local_digest = fetch_local_wikitext_sha256(
                    local_base, entry["title"], local_identity["page_id"],
                    local_identity["revision_id"])
                row["local_wikitext_sha256"] = local_digest
                row["source_wikitext_match"] = local_digest == source_digest
            else:
                row["source_wikitext_match"] = None
            if local_summary["error_markers"]:
                row["local_error_details"] = local_summary["error_markers"]
                row["local_error_excerpt"] = _error_excerpt(local_html)
            if live_summary["error_markers"]:
                row["live_error_details"] = live_summary["error_markers"]
                row["live_error_excerpt"] = _error_excerpt(live_html)
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
        if (metrics["local_error_markers"] or metrics["live_error_markers"]
                or metrics["link_jaccard"] < 0.5
                or row.get("source_timestamp_match") is False
                or row.get("source_wikitext_match") is not True):
            print("FIDELITY-WARNING:", json.dumps({
                "title": row["title"], "source_revision_id": row["source_revision_id"],
                "link_jaccard": metrics["link_jaccard"],
                "missing_link_count": metrics["missing_link_count"],
                "unexpected_link_count": metrics["unexpected_link_count"],
                "missing_links_sample": metrics["missing_links_sample"],
                "unexpected_links_sample": metrics["unexpected_links_sample"],
                "local_error_details": row.get("local_error_details", {}),
                "local_error_excerpt": row.get("local_error_excerpt", ""),
                "live_error_details": row.get("live_error_details", {}),
                "live_error_excerpt": row.get("live_error_excerpt", ""),
                "error_marker_delta": metrics["error_marker_delta"],
                "source_timestamp_match": row.get("source_timestamp_match"),
                "local_id_comparison": row.get("local_id_comparison", {}),
                "source_page_id": row.get("source_page_id"),
                "source_wikitext_match": row.get("source_wikitext_match"),
                "local_wikitext_sha256": row.get("local_wikitext_sha256"),
                "local_identity": row.get("local_identity", {}),
                "redlink_status_drift_count": metrics["redlink_status_drift_count"],
                "image_count_delta": metrics["image_count_delta"],
                "mathml_count_delta": metrics["mathml_count_delta"],
                "local_visuals": metrics["local_visuals"],
            }, ensure_ascii=False, sort_keys=True), file=sys.stderr)
    for failure in report["failures"]:
        print("FAIL:", failure, file=sys.stderr)
    return 1 if report["failures"] else 0


if __name__ == "__main__":
    sys.exit(_main(sys.argv[1:]))
