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
import xml.etree.ElementTree as ET

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


MAX_SAMPLE_PAGE_XML_BYTES = 64 * 1024 * 1024
# Bound *every* page, including non-sampled template/module pages, before
# retaining a potentially huge XML block for diagnostic rendering.
MAX_PAGE_XML_BYTES = 64 * 1024 * 1024
# Limit preamble/siteinfo allocation independently of individual page size.
MAX_HEADER_XML_BYTES = 8 * 1024 * 1024


def _sample_wikitext_sha256(block, declared_bytes):
    """Decode a sampled original XML revision and hash its actual UTF-8 text.

    The standalone page XML is extracted from the already verified source
    member; the whole-block bound prevents parser memory growth.
    """
    markup = "".join(block)
    if len(markup.encode("utf-8")) > MAX_SAMPLE_PAGE_XML_BYTES:
        raise DumpFormatError("sample page XML exceeds bounded decode limit")
    try:
        page = ET.fromstring(markup)
    except ET.ParseError as error:
        raise DumpFormatError("malformed sampled page XML") from error
    if page.tag != "page":
        raise DumpFormatError("unexpected sampled XML root")
    revisions = page.findall("revision")
    if len(revisions) != 1:
        raise DumpFormatError("ambiguous sampled revision")
    texts = revisions[0].findall("text")
    if len(texts) != 1 or len(texts[0]):
        raise DumpFormatError("missing or nested sampled wikitext")
    wikitext = texts[0].text or ""
    encoded = wikitext.encode("utf-8")
    if len(encoded) != declared_bytes:
        raise DumpFormatError("sampled wikitext declared byte length mismatch")
    return hashlib.sha256(encoded).hexdigest()


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
                sample_min_bytes=SAMPLE_MIN_BYTES, max_page_xml_bytes=MAX_PAGE_XML_BYTES,
                max_header_xml_bytes=MAX_HEADER_XML_BYTES):
    """Stream ``lines`` to ``out``; return a summary with the article sample."""
    if not isinstance(max_page_xml_bytes, int) or max_page_xml_bytes < 1:
        raise DumpFormatError("invalid page XML byte cap")
    if not isinstance(max_header_xml_bytes, int) or max_header_xml_bytes < 1:
        raise DumpFormatError("invalid header XML byte cap")
    keep = set(keep_namespaces)
    iterator = iter(lines)
    seen_root = False
    header_bytes = 0
    for line in iterator:
        if line == "  <page>\n":
            break
        header_bytes += len(line.encode("utf-8"))
        if header_bytes > max_header_xml_bytes:
            raise DumpFormatError("header XML exceeds byte cap")
        if line.startswith("<mediawiki "):
            if seen_root:
                raise DumpFormatError("duplicate mediawiki root")
            seen_root = True
        out.write(line)
    else:
        raise DumpFormatError("no <page> element found")
    if not seen_root:
        raise DumpFormatError("missing <mediawiki> root element")
    summary = {"source_pages": 0, "kept_by_namespace": {}, "sample": []}
    block = [line]
    block_bytes = len(line.encode("utf-8"))
    if block_bytes > max_page_xml_bytes:
        raise DumpFormatError("page XML exceeds byte cap")
    footer_seen = False
    for line in iterator:
        if line == "  </page>\n":
            block_bytes += len(line.encode("utf-8"))
            if block_bytes > max_page_xml_bytes:
                raise DumpFormatError("page XML exceeds byte cap")
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
                sample = {k: record[k] for k in ("title", "page_id", "revision_id", "timestamp", "bytes")}
                sample["source_wikitext_sha256"] = _sample_wikitext_sha256(block, record["bytes"])
                summary["sample"].append(sample)
            block = None
            block_bytes = 0
        elif block is None:
            if footer_seen:
                raise DumpFormatError("content after closing mediawiki root")
            if line == "  <page>\n":
                block = [line]
                block_bytes = len(line.encode("utf-8"))
                if block_bytes > max_page_xml_bytes:
                    raise DumpFormatError("page XML exceeds byte cap")
            elif line == "</mediawiki>\n":
                out.write(line)
                footer_seen = True
            elif line.strip():
                raise DumpFormatError(f"unexpected content between pages: {line[:60]!r}")
        else:
            block_bytes += len(line.encode("utf-8"))
            if block_bytes > max_page_xml_bytes:
                raise DumpFormatError("page XML exceeds byte cap")
            block.append(line)
    if block is not None or not footer_seen:
        raise DumpFormatError("truncated export: missing closing </page> or </mediawiki>")
    if not summary["sample"]:
        raise DumpFormatError("no article matched the sample criteria")
    return summary


HASH_CHUNK_BYTES = 1024 * 1024
# The diagnostic importer is deliberately bounded to one reasonably sized
# XML line. All UTF-8 page/header byte budgets are checked independently.
MAX_INPUT_LINE_CHARS = 64 * 1024 * 1024


def _bounded_lines(stream, max_line_chars=MAX_INPUT_LINE_CHARS):
    """Avoid TextIO's unbounded line iterator on malformed huge XML lines."""
    if not isinstance(max_line_chars, int) or max_line_chars < 1:
        raise DumpFormatError("invalid input line character cap")
    while True:
        line = stream.readline(max_line_chars + 1)
        if not line:
            return
        if len(line) > max_line_chars:
            raise DumpFormatError("input XML line exceeds character cap")
        yield line


def _sha256_stream(stream):
    """Bound filtered XML checksum memory regardless of dump export size."""
    digest = hashlib.sha256()
    while True:
        block = stream.read(HASH_CHUNK_BYTES)
        if not block:
            return digest.hexdigest()
        digest.update(block)


def _main(argv):
    if len(argv) != 3:
        print("usage: filter_dump_for_render.py INPUT.xml[.bz2] OUTPUT.xml SAMPLE.json", file=sys.stderr)
        return 2
    source, target, sample_path = argv
    opener = bz2.open if source.endswith(".bz2") else open
    with opener(source, "rt", encoding="utf-8", newline="\n") as handle, \
            open(target, "w", encoding="utf-8", newline="\n") as out:
        summary = filter_dump(_bounded_lines(handle), out)
    with open(target, "rb") as handle:
        summary["filtered_sha256"] = _sha256_stream(handle)
    with open(sample_path, "w", encoding="utf-8") as handle:
        json.dump(summary, handle, ensure_ascii=False, indent=2, sort_keys=True)
        handle.write("\n")
    print(json.dumps({k: v for k, v in summary.items() if k != "sample"}, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(_main(sys.argv[1:]))
