#!/usr/bin/env python3
"""Diagnostic current-content XML/bzip2 -> observed page-ID census.

The manifest and its digests MUST be authenticated independently. This helper
is not the Rust importer, a source-completeness oracle, or an HTML renderer.
It publishes only after all declared members decode and validate successfully.
"""
import bz2
import json
import os
import re
import tempfile
import xml.etree.ElementTree as ET
from pathlib import Path

from verify_source_staging import SourceVerificationError, verify_source_bytes

NAME = re.compile(r"^(?P<project>[a-z0-9_]+)-(?P<date>\d{4}-\d{2}-\d{2})-p(?P<first>[1-9]\d*)p(?P<last>[1-9]\d*)\.xml\.bz2$")
MAX_XML_BYTES = 16 * 1024**3
MAX_PAGE_ID = 2**63 - 1


class DumpDecodeError(ValueError):
    """Invalid dump input; never publish a partial observed census."""


def _tag(tag):
    return tag.rsplit("}", 1)[-1]


def _one(parent, name):
    found = [child for child in parent if _tag(child.tag) == name]
    if len(found) != 1:
        raise DumpDecodeError(f"expected exactly one {name}")
    return found[0]


def _positive(text):
    if text is None or not re.fullmatch(r"[1-9][0-9]*", text):
        raise DumpDecodeError("invalid positive page/revision ID")
    value = int(text)
    if value > MAX_PAGE_ID:
        raise DumpDecodeError("page/revision ID exceeds SQLite range")
    return value


def _page_id(page):
    title = _one(page, "title").text
    if not title or len(title.encode("utf-8")) > 4096 or title != title.strip():
        raise DumpDecodeError("invalid page title")
    namespace = _one(page, "ns").text
    if namespace is None or not re.fullmatch(r"0|[1-9][0-9]*", namespace):
        raise DumpDecodeError("invalid namespace")
    page_id = _positive(_one(page, "id").text)
    revision = _one(page, "revision")
    _positive(_one(revision, "id").text)
    timestamp = _one(revision, "timestamp").text
    if not timestamp or not re.fullmatch(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z", timestamp):
        raise DumpDecodeError("invalid revision timestamp")
    return page_id, int(namespace)


class BoundedDecompressed:
    """Limit decompressed input and reject XML DTD/entity declarations."""
    def __init__(self, stream):
        self.stream = stream
        self.total = 0
        self.tail = b""

    def read(self, size=-1):
        if size < 0:
            size = 65536
        data = self.stream.read(min(size, 65536))
        self.total += len(data)
        if self.total > MAX_XML_BYTES:
            raise DumpDecodeError("decompressed member exceeds limit")
        joined = self.tail + data
        if b"<!DOCTYPE" in joined or b"<!ENTITY" in joined:
            raise DumpDecodeError("DTD/entity declarations are forbidden")
        self.tail = joined[-8:]
        return data


def decode_member(path, first, last, output, namespace):
    """Stream a complete, well-formed bzip2 XML member to a pending sink."""
    previous = first - 1
    pages = 0
    selected = 0
    try:
        with bz2.open(path, "rb") as stream:
            events = ET.iterparse(BoundedDecompressed(stream), events=("start", "end"))
            depth = 0
            root = None
            for event, elem in events:
                if event == "start":
                    depth += 1
                    if depth == 1:
                        if _tag(elem.tag) != "mediawiki":
                            raise DumpDecodeError("expected mediawiki XML root")
                        root = elem
                    continue
                if _tag(elem.tag) == "page" and depth != 2:
                    raise DumpDecodeError("page outside mediawiki root")
                if depth == 2 and _tag(elem.tag) == "page":
                    page_id, ns = _page_id(elem)
                    if not first <= page_id <= last or page_id <= previous:
                        raise DumpDecodeError("page ID out of shard range or not strictly increasing")
                    previous = page_id
                    pages += 1
                    if ns == namespace:
                        output.write(f"{page_id}\n".encode("ascii"))
                        selected += 1
                    elem.clear()
                    root.clear()
                depth -= 1
            if depth != 0 or root is None or pages == 0:
                raise DumpDecodeError("empty or incomplete XML member")
    except (ET.ParseError, EOFError, OSError, UnicodeError) as exc:
        raise DumpDecodeError("malformed or truncated bzip2/XML member") from exc
    return selected


def stage_observed_census(manifest, staging, output_dir, filename, namespace=0):
    """Verify all staged digests, decode all shards, then publish atomically."""
    if type(namespace) is not int or namespace < 0:
        raise DumpDecodeError("invalid namespace")
    if not re.fullmatch(r"[A-Za-z0-9_-][A-Za-z0-9_.-]*", filename) or filename.endswith("."):
        raise DumpDecodeError("invalid output filename")
    output_dir = Path(output_dir)
    if any(p.is_symlink() for p in (output_dir, *output_dir.parents)):
        raise DumpDecodeError("symlinked output directory")
    if not output_dir.is_dir() or (os.name == "posix" and output_dir.stat().st_mode & 0o077):
        raise DumpDecodeError("output directory must be private")
    try:
        verify_source_bytes(manifest, staging)
    except SourceVerificationError as exc:
        raise DumpDecodeError("source staging verification failed") from exc
    shards = []
    for item in manifest["files"]:
        match = NAME.fullmatch(item["name"])
        if not match or match["project"] != manifest["project"] or match["date"] != manifest["generation_id"]:
            raise DumpDecodeError("unrecognized current-content member name")
        first, last = int(match["first"]), int(match["last"])
        if not 0 < first <= last <= MAX_PAGE_ID:
            raise DumpDecodeError("invalid declared shard range")
        shards.append((first, last, item["name"]))
    shards.sort()
    for previous, current in zip(shards, shards[1:]):
        if current[0] <= previous[1]:
            raise DumpDecodeError("overlapping shard ranges")
    fd, pending = tempfile.mkstemp(prefix=".wiki-census-", dir=output_dir)
    published = False
    destination = output_dir / filename
    try:
        with os.fdopen(fd, "wb") as sink:
            selected = 0
            for first, last, name in shards:
                selected += decode_member(Path(staging) / name, first, last, sink, namespace)
            if not selected:
                raise DumpDecodeError("selected namespace has no pages")
            sink.flush()
            os.fsync(sink.fileno())
        os.link(pending, destination)  # no-overwrite, same filesystem
        published = True
        os.unlink(pending)
        pending = None
        if os.name == "posix":
            directory_fd = os.open(output_dir, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
            try:
                os.fsync(directory_fd)
            finally:
                os.close(directory_fd)
        return destination, selected
    except BaseException:
        if published:
            os.unlink(destination)
        raise
    finally:
        if pending is not None:
            os.unlink(pending)


def main():
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("staging", type=Path)
    parser.add_argument("output_dir", type=Path)
    parser.add_argument("filename")
    parser.add_argument("--namespace", type=int, default=0)
    args = parser.parse_args()
    result, count = stage_observed_census(json.loads(args.manifest.read_text()), args.staging, args.output_dir, args.filename, args.namespace)
    print(f"Observed {count} page IDs in {result}; publisher authenticity and completeness NOT established")


if __name__ == "__main__":
    main()
