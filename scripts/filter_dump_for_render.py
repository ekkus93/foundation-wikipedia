#!/usr/bin/env python3
"""Cut a verified current-content XML export down to what a render check needs.

Keeps the export header, every page in the dependency namespaces (default
MediaWiki, Template and Module), and a deterministic sample of non-redirect
articles that use templates and references. The input must be the pretty-printed
export-0.11 layout with exactly one revision per page; anything else fails
closed rather than silently dropping content.
"""
import bz2
import hashlib
import html
import json
import re
import sys

KEEP_NAMESPACES = (8, 10, 828)
SAMPLE_LIMIT = 25
SAMPLE_MIN_BYTES = 3000

_NS = re.compile(r"^    <ns>(-?\d+)</ns>$")
_TITLE = re.compile(r"^    <title>(.*)</title>$")
_PAGE_ID = re.compile(r"^    <id>(\d+)</id>$")
_REV_ID = re.compile(r"^      <id>(\d+)</id>$")
_TIMESTAMP = re.compile(r"^      <timestamp>([^<]+)</timestamp>$")
_TEXT_BYTES = re.compile(r'^      <text bytes="(\d+)"')


class DumpFormatError(ValueError):
    pass


def _page_record(block):
    """Parse the identity of one ``<page>`` block; fail on unexpected shape."""
    record = {"redirect": False, "revisions": 0}
    for line in block:
        text = line.rstrip("\n")
        for key, pattern in (("ns", _NS), ("title", _TITLE), ("page_id", _PAGE_ID)):
            match = pattern.match(text)
            if match and key not in record:
                record[key] = match.group(1)
        if text == "    <revision>":
            record["revisions"] += 1
        elif text.startswith("    <redirect "):
            record["redirect"] = True
        elif record["revisions"] == 1:
            for key, pattern in (("revision_id", _REV_ID), ("timestamp", _TIMESTAMP), ("bytes", _TEXT_BYTES)):
                match = pattern.match(text)
                if match and key not in record:
                    record[key] = match.group(1)
    if record["revisions"] != 1:
        raise DumpFormatError(f"page {record.get('title')!r} has {record['revisions']} revisions")
    for key in ("ns", "title", "page_id", "revision_id", "timestamp", "bytes"):
        if key not in record:
            raise DumpFormatError(f"page {record.get('title')!r} is missing {key}")
    record["ns"] = int(record["ns"])
    record["page_id"] = int(record["page_id"])
    record["revision_id"] = int(record["revision_id"])
    record["bytes"] = int(record["bytes"])
    record["title"] = html.unescape(record["title"])
    return record


def filter_dump(lines, out, keep_namespaces=KEEP_NAMESPACES, sample_limit=SAMPLE_LIMIT,
                sample_min_bytes=SAMPLE_MIN_BYTES):
    """Stream ``lines`` to ``out``; return a summary with the article sample."""
    keep = set(keep_namespaces)
    iterator = iter(lines)
    seen_root = False
    for line in iterator:
        if line == "  <page>\n":
            break
        if line.startswith("<mediawiki "):
            seen_root = True
        out.write(line)
    else:
        raise DumpFormatError("no <page> element found")
    if not seen_root:
        raise DumpFormatError("missing <mediawiki> root element")
    summary = {"source_pages": 0, "kept_by_namespace": {}, "sample": []}
    block = [line]
    footer_seen = False
    for line in iterator:
        if line == "  </page>\n":
            block.append(line)
            record = _page_record(block)
            summary["source_pages"] += 1
            body = "".join(block)
            is_sample = (
                record["ns"] == 0
                and not record["redirect"]
                and record["bytes"] >= sample_min_bytes
                and "{{" in body
                and "&lt;ref" in body
                and len(summary["sample"]) < sample_limit
            )
            if record["ns"] in keep or is_sample:
                out.write(body)
                key = str(record["ns"])
                summary["kept_by_namespace"][key] = summary["kept_by_namespace"].get(key, 0) + 1
            if is_sample:
                summary["sample"].append({k: record[k] for k in ("title", "page_id", "revision_id", "timestamp", "bytes")})
            block = None
        elif block is None:
            if line == "  <page>\n":
                block = [line]
            elif line == "</mediawiki>\n":
                out.write(line)
                footer_seen = True
            elif line.strip():
                raise DumpFormatError(f"unexpected content between pages: {line[:60]!r}")
        else:
            block.append(line)
    if block is not None or not footer_seen:
        raise DumpFormatError("truncated export: missing closing </page> or </mediawiki>")
    if not summary["sample"]:
        raise DumpFormatError("no article matched the sample criteria")
    return summary


def _main(argv):
    if len(argv) != 3:
        print("usage: filter_dump_for_render.py INPUT.xml[.bz2] OUTPUT.xml SAMPLE.json", file=sys.stderr)
        return 2
    source, target, sample_path = argv
    opener = bz2.open if source.endswith(".bz2") else open
    digest = hashlib.sha256()
    with opener(source, "rt", encoding="utf-8", newline="\n") as handle, \
            open(target, "w", encoding="utf-8", newline="\n") as out:
        summary = filter_dump(handle, out)
    with open(target, "rb") as handle:
        digest.update(handle.read())
    summary["filtered_sha256"] = digest.hexdigest()
    with open(sample_path, "w", encoding="utf-8") as handle:
        json.dump(summary, handle, ensure_ascii=False, indent=2, sort_keys=True)
        handle.write("\n")
    print(json.dumps({k: v for k, v in summary.items() if k != "sample"}, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(_main(sys.argv[1:]))
