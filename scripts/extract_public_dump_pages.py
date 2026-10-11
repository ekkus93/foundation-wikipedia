#!/usr/bin/env python3
"""Stage revision-scoped page records from a verified public current-content XML shard.

This is a bounded *wikitext extraction* step for SRC-003, not MediaWiki/Parsoid
rendering, canonical article normalization, pack signing, or snapshot activation.
The caller must authenticate the source publication independently. A matching
caller-supplied SHA-256 alone does not prove publisher authenticity.
"""
import argparse
import bz2
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import tempfile
import xml.etree.ElementTree as ET


class DumpExtractionError(ValueError):
    """A shard cannot safely be interpreted as current-content page records."""


def _child(parent, name):
    """Return one direct scalar element; never silently discard nested XML."""
    matches = parent.findall("{*}" + name)
    if len(matches) > 1:
        raise DumpExtractionError(f"duplicate {name} element")
    if matches and len(matches[0]):
        raise DumpExtractionError(f"nested XML in {name} element")
    return matches[0] if matches else None


def _text(parent, name):
    element = _child(parent, name)
    return element.text if element is not None else None


def _positive_id(value):
    try:
        result = int(value)
    except (TypeError, ValueError):
        raise DumpExtractionError("missing or invalid page/revision identity") from None
    if result <= 0 or result > 2**63 - 1:
        raise DumpExtractionError("invalid page/revision identity")
    return result


class _BoundedXmlReader:
    """Reject decompression bombs, XML DTDs/entities and non-UTF8 encodings."""

    def __init__(self, reader, maximum):
        self.reader = reader
        self.maximum = maximum
        self.count = 0
        self.tail = b""
        self.first = True

    def read(self, size=-1):
        if size < 0:
            size = 16384
        data = self.reader.read(min(size, 16384, self.maximum - self.count + 1))
        if self.first:
            self.first = False
            if not (data.startswith(b"<?xml") or data.startswith(b"<mediawiki")
                    or data.startswith(b"\xef\xbb\xbf<?xml")):
                raise DumpExtractionError("expected UTF-8 MediaWiki XML")
            # ElementTree can decode legacy single-byte encodings. An offline
            # canonical importer must not silently accept non-UTF-8 source text.
            if data.startswith(b"<?xml") or data.startswith(b"\xef\xbb\xbf<?xml"):
                declaration = data[3:] if data.startswith(b"\xef\xbb\xbf") else data
                header, separator, _ = declaration.partition(b"?>")
                if not separator:
                    raise DumpExtractionError("invalid XML declaration")
                encoding = re.search(
                    rb"""\bencoding\s*=\s*(['"])([^'"]+)\1""", header, re.IGNORECASE
                )
                if encoding and encoding.group(2).lower() != b"utf-8":
                    raise DumpExtractionError("expected UTF-8 MediaWiki XML")
        self.count += len(data)
        if self.count > self.maximum:
            raise DumpExtractionError("decompressed XML exceeds budget")
        window = self.tail + data
        if b"<!DOCTYPE" in window or b"<!ENTITY" in window:
            raise DumpExtractionError("XML DTD/entity declarations are forbidden")
        self.tail = window[-16:]
        return data


def _source_identity(metadata):
    return (metadata.st_dev, metadata.st_ino, metadata.st_size,
            metadata.st_mtime_ns, metadata.st_ctime_ns, metadata.st_nlink)


def _verified_source(path, expected_sha256):
    """Keep the verified source descriptor open through extraction."""
    if not re.fullmatch(r"[0-9a-fA-F]{64}", expected_sha256):
        raise DumpExtractionError("invalid expected SHA-256")
    if path.is_symlink():
        raise DumpExtractionError("symlinked source shard")
    try:
        descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
                             | getattr(os, "O_NONBLOCK", 0))
    except OSError as error:
        raise DumpExtractionError("could not safely open source shard") from error
    source = os.fdopen(descriptor, "rb")
    try:
        metadata = os.fstat(source.fileno())
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
            raise DumpExtractionError("source must be a private regular file")
        if _source_identity(path.lstat()) != _source_identity(metadata):
            raise DumpExtractionError("source changed during validation")
        digest = hashlib.sha256()
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
        if digest.hexdigest() != expected_sha256.lower():
            raise DumpExtractionError("compressed source SHA-256 mismatch")
        source.seek(0)
        return source, metadata
    except BaseException:
        source.close()
        raise


def _verify_unchanged_source(path, source, initial, expected_sha256):
    """Recheck the opened bytes and pathname identity before publication."""
    try:
        metadata = path.lstat()
    except OSError as error:
        raise DumpExtractionError("source changed during extraction") from error
    if (_source_identity(os.fstat(source.fileno())) != _source_identity(initial)
            or _source_identity(metadata) != _source_identity(initial)
            or not stat.S_ISREG(metadata.st_mode)):
        raise DumpExtractionError("source changed during extraction")
    source.seek(0)
    digest = hashlib.sha256()
    for chunk in iter(lambda: source.read(1024 * 1024), b""):
        digest.update(chunk)
    if digest.hexdigest() != expected_sha256.lower():
        raise DumpExtractionError("source changed during extraction")


def _page_record(page, project, source_sha256, max_text_bytes):
    revisions = page.findall("{*}revision")
    if len(revisions) != 1:
        raise DumpExtractionError("current-content page must have one revision")
    revision = revisions[0]
    title = _text(page, "title")
    timestamp = _text(revision, "timestamp")
    text_node = _child(revision, "text")
    if not title or not title.strip() or not timestamp or text_node is None:
        raise DumpExtractionError("missing page title, revision time or wikitext")
    if any(key not in ("bytes", "{http://www.w3.org/XML/1998/namespace}space")
           for key in text_node.attrib):
        raise DumpExtractionError("unsupported revision text attributes")
    body = text_node.text or ""
    body_bytes = len(body.encode("utf-8"))
    declared_bytes = text_node.get("bytes")
    if declared_bytes is not None and (
        not re.fullmatch(r"(0|[1-9][0-9]*)", declared_bytes)
        or int(declared_bytes) != body_bytes
    ):
        raise DumpExtractionError("revision text byte count mismatch")
    if len(title.encode("utf-8")) > 4096 or body_bytes > max_text_bytes:
        raise DumpExtractionError("oversized page title or wikitext")
    try:
        namespace = int(_text(page, "ns"))
    except (ValueError, TypeError):
        raise DumpExtractionError("missing page namespace") from None
    redirect = _child(page, "redirect")
    target = redirect.get("title") if redirect is not None else None
    if redirect is not None and (not target or not target.strip()):
        raise DumpExtractionError("redirect missing target")
    return {
        "project": project,
        "page_id": _positive_id(_text(page, "id")),
        "revision_id": _positive_id(_text(revision, "id")),
        "timestamp": timestamp,
        "namespace": namespace,
        "title": title,
        "redirect_target": target,
        "wikitext": body,
        "source_sha256": source_sha256.lower(),
    }


def extract_current_dump(path, expected_sha256, project, output, *,
                         max_uncompressed_bytes=512 * 1024 * 1024,
                         max_pages=1_000_000, max_text_bytes=8 * 1024 * 1024):
    """Verify compressed bytes, stream one shard, atomically publish JSONL."""
    path, output = Path(path), Path(output)
    if not re.fullmatch(r"[a-z0-9_]+", project):
        raise DumpExtractionError("invalid wiki project")
    if not (0 < max_uncompressed_bytes <= 2**34
            and 0 < max_pages <= 10_000_000
            and 0 < max_text_bytes <= 64 * 1024 * 1024):
        raise DumpExtractionError("invalid extraction budget")
    if output.parent.is_symlink() or not output.parent.is_dir():
        raise DumpExtractionError("unsafe output parent")
    if output.exists() or output.is_symlink():
        raise DumpExtractionError("output already exists")
    partition = re.fullmatch(
        r"([a-z0-9_]+)-[0-9]{4}-[0-9]{2}-01-p([0-9]+)p([0-9]+)\.xml\.bz2",
        path.name,
    )
    if partition is None or partition.group(1) != project:
        raise DumpExtractionError("unexpected current-export member identity")
    first_id, last_id = int(partition.group(2)), int(partition.group(3))
    if not 0 < first_id <= last_id:
        raise DumpExtractionError("invalid page-ID partition")
    source, original_metadata = _verified_source(path, expected_sha256)
    temporary = None
    count = 0
    seen = set()
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=output.parent,
                                         prefix=".dump-pages.", suffix=".tmp",
                                         delete=False) as writer:
            temporary = Path(writer.name)
            with bz2.BZ2File(source, "rb") as compressed:
                reader = _BoundedXmlReader(compressed, max_uncompressed_bytes)
                root = None
                try:
                    for event, element in ET.iterparse(reader, events=("start", "end")):
                        tag = element.tag.rsplit("}", 1)[-1]
                        if root is None:
                            if event != "start" or tag != "mediawiki":
                                raise DumpExtractionError("not a MediaWiki XML export")
                            root = element
                        if event == "end" and tag == "page":
                            record = _page_record(element, project, expected_sha256,
                                                  max_text_bytes)
                            if not first_id <= record["page_id"] <= last_id:
                                raise DumpExtractionError("page ID outside declared shard partition")
                            if record["page_id"] in seen:
                                raise DumpExtractionError("duplicate page ID in shard")
                            seen.add(record["page_id"])
                            count += 1
                            if count > max_pages:
                                raise DumpExtractionError("page count exceeds budget")
                            json.dump(record, writer, ensure_ascii=False, sort_keys=True)
                            writer.write("\n")
                            root.clear()
                except ET.ParseError as error:
                    raise DumpExtractionError("malformed MediaWiki XML") from error
                if root is None or count == 0:
                    raise DumpExtractionError("empty MediaWiki XML export")
            writer.flush()
            os.fsync(writer.fileno())
        _verify_unchanged_source(path, source, original_metadata, expected_sha256)
        try:
            os.link(temporary, output)
        except FileExistsError as error:
            raise DumpExtractionError("output already exists") from error
        directory = os.open(output.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
        return count
    finally:
        source.close()
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("compressed_xml", type=Path)
    parser.add_argument("expected_sha256")
    parser.add_argument("project")
    parser.add_argument("output_jsonl", type=Path)
    args = parser.parse_args()
    count = extract_current_dump(args.compressed_xml, args.expected_sha256,
                                 args.project, args.output_jsonl)
    print(f"Extracted {count} unrendered page record(s); NOT an offline-ready pack.")


if __name__ == "__main__":
    main()
