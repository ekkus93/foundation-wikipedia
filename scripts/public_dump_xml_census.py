#!/usr/bin/env python3
"""Diagnostic current-content XML/bzip2 -> observed page-ID census.

The manifest and its digests MUST be authenticated independently. This helper
is not the Rust importer, a source-completeness oracle, or an HTML renderer.
It publishes only after all declared members decode and validate successfully.
"""
import bz2
from datetime import datetime
import hashlib
import json
import os
import re
import stat
import tempfile
import xml.etree.ElementTree as ET
from pathlib import Path

from audit_public_dump_revision_jsonl import RevisionAuditError, audit_revision_jsonl
from verify_source_staging import SourceVerificationError, load_source_manifest, verify_source_bytes

EXPORT_NS = re.compile(r"^http://www\.mediawiki\.org/xml/export-[0-9]+\.[0-9]+/$")
NAME = re.compile(r"^(?P<project>[a-z0-9_]+)-(?P<date>\d{4}-\d{2}-\d{2})-p(?P<first>[1-9]\d*)p(?P<last>[1-9]\d*)\.xml\.bz2$")
MAX_XML_BYTES = 16 * 1024**3
# Bound the ElementTree subtree retained for any one page (including wikitext).
MAX_PAGE_XML_BYTES = 32 * 1024**2
# Keep non-page XML (such as <siteinfo>) from growing an unbounded tree.
MAX_NON_PAGE_XML_BYTES = 8 * 1024**2
MAX_XML_DEPTH = 128
MAX_PAGE_ID = 2**63 - 1
MAX_NAMESPACE_ID = 2**31 - 1


class DumpDecodeError(ValueError):
    """Invalid dump input; never publish a partial observed census."""


def _tag(tag):
    return tag.rsplit("}", 1)[-1]


def _namespace(tag):
    return tag[1:].split("}", 1)[0] if tag.startswith("{") else ""


def _one(parent, name):
    namespace = _namespace(parent.tag)
    expected = f"{{{namespace}}}{name}" if namespace else name
    found = [child for child in parent if child.tag == expected]
    if len(found) != 1:
        raise DumpDecodeError(f"expected exactly one {name}")
    return found[0]


def _positive(text):
    if text is None or len(text) > 19 or not re.fullmatch(r"[1-9][0-9]*", text):
        raise DumpDecodeError("invalid positive page/revision ID")
    value = int(text)
    if value > MAX_PAGE_ID:
        raise DumpDecodeError("page/revision ID exceeds SQLite range")
    return value


def _page_identity(page):
    title = _one(page, "title").text
    if (not title or len(title.encode("utf-8")) > 4096 or title != title.strip()
            or any(ord(char) < 32 or ord(char) == 127 for char in title)):
        raise DumpDecodeError("invalid page title")
    namespace = _one(page, "ns").text
    if namespace is None or len(namespace) > 10 or not re.fullmatch(r"0|[1-9][0-9]*", namespace):
        raise DumpDecodeError("invalid namespace")
    namespace_id = int(namespace)
    if namespace_id > MAX_NAMESPACE_ID:
        raise DumpDecodeError("namespace exceeds supported range")
    page_id = _positive(_one(page, "id").text)
    revision = _one(page, "revision")
    revision_id = _positive(_one(revision, "id").text)
    timestamp = _one(revision, "timestamp").text
    if not timestamp or not re.fullmatch(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z", timestamp):
        raise DumpDecodeError("invalid revision timestamp")
    try:
        datetime.strptime(timestamp, "%Y-%m-%dT%H:%M:%SZ")
    except ValueError as exc:
        raise DumpDecodeError("invalid revision calendar timestamp") from exc
    expected_redirect = f"{{{_namespace(page.tag)}}}redirect" if _namespace(page.tag) else "redirect"
    redirects = [child for child in page if child.tag == expected_redirect]
    if len(redirects) > 1:
        raise DumpDecodeError("multiple redirect targets")
    redirect_target = None
    if redirects:
        redirect = redirects[0]
        if set(redirect.attrib) != {"title"}:
            raise DumpDecodeError("invalid redirect attributes")
        redirect_target = redirect.attrib["title"]
        if (not redirect_target or redirect_target != redirect_target.strip()
                or len(redirect_target.encode("utf-8")) > 4096
                or any(ord(char) < 32 or ord(char) == 127 for char in redirect_target)):
            raise DumpDecodeError("invalid redirect title")
    return page_id, namespace_id, title, revision_id, timestamp, redirect_target


def _revision_wikitext(revision):
    """Extract one available main-slot wikitext body, not rendered HTML.

    Modern dumps may wrap content under a role=main slot, while older
    current-content exports use revision/text directly. Do not concatenate
    other content slots or claim exact original XML byte equivalence.
    """
    namespace = _namespace(revision.tag)
    def child_name(name):
        return f"{{{namespace}}}{name}" if namespace else name

    direct = [item for item in revision if item.tag == child_name("text")]
    slots = [item for item in revision if item.tag == child_name("slots")]
    if len(direct) == 1 and not slots:
        text = direct[0]
    elif not direct and len(slots) == 1:
        candidates = [
            item for item in slots[0]
            if item.tag == child_name("slot") and item.attrib.get("role") == "main"
        ]
        if len(candidates) != 1:
            raise DumpDecodeError("missing or duplicate main revision slot")
        text = _one(candidates[0], "text")
    else:
        raise DumpDecodeError("missing or ambiguous revision wikitext")
    if "deleted" in text.attrib or len(text):
        raise DumpDecodeError("unavailable or invalid revision wikitext")
    return text.text or ""


class BoundedDecompressed:
    """Limit decompressed input and reject XML DTD/entity declarations."""
    def __init__(self, stream):
        self.stream = stream
        self.total = 0
        self.tail = b""
        self.page_start = None
        self.non_page_start = 0

    def read(self, size=-1):
        if size < 0:
            size = 65536
        # Small reads also make a lowered test limit meaningful when one XML
        # page otherwise fits in the parser's first input buffer.
        data = self.stream.read(min(size, 65536, max(1, MAX_PAGE_XML_BYTES // 4)))
        self.total += len(data)
        if self.total > MAX_XML_BYTES:
            raise DumpDecodeError("decompressed member exceeds limit")
        if self.page_start is not None and self.total - self.page_start > MAX_PAGE_XML_BYTES:
            raise DumpDecodeError("XML page exceeds per-page memory limit")
        if self.page_start is None and self.total - self.non_page_start > MAX_NON_PAGE_XML_BYTES:
            raise DumpDecodeError("non-page XML exceeds metadata memory limit")
        joined = self.tail + data
        if b"<!DOCTYPE" in joined or b"<!ENTITY" in joined:
            raise DumpDecodeError("DTD/entity declarations are forbidden")
        self.tail = joined[-8:]
        return data


class HashingCompressed:
    """Hash the exact compressed bytes consumed by the XML decoder."""
    def __init__(self, stream):
        self.stream = stream
        self.digest = hashlib.sha256()
        self.size = 0

    def read(self, size=-1):
        data = self.stream.read(size)
        self.digest.update(data)
        self.size += len(data)
        return data


def decode_member(path, first, last, output, namespace, expected_sha256, expected_size,
                  output_format="ids", project=None, generation=None):
    """Stream a complete, well-formed bzip2 XML member to a pending sink."""
    previous = first - 1
    pages = 0
    selected = 0
    try:
        flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0)
        with os.fdopen(os.open(path, flags), "rb") as handle:
            before = os.fstat(handle.fileno())
            if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or before.st_size != expected_size:
                raise DumpDecodeError("unsafe or replaced compressed member")
            compressed = HashingCompressed(handle)
            with bz2.BZ2File(compressed) as stream:
                bounded = BoundedDecompressed(stream)
                events = ET.iterparse(bounded, events=("start", "end"))
                depth = 0
                root = None
                seen_siteinfo = False
                seen_page = False
                for event, elem in events:
                    if event == "start":
                        depth += 1
                        if depth > MAX_XML_DEPTH:
                            raise DumpDecodeError("XML nesting exceeds depth limit")
                        if depth == 1:
                            if (_tag(elem.tag) != "mediawiki"
                                    or (_namespace(elem.tag) and not EXPORT_NS.fullmatch(_namespace(elem.tag)))):
                                raise DumpDecodeError("expected official mediawiki XML root")
                            root = elem
                        if depth == 2:
                            if _tag(elem.tag) == "page" and _namespace(elem.tag) != _namespace(root.tag):
                                raise DumpDecodeError("page outside mediawiki root namespace")
                            if (_namespace(elem.tag) != _namespace(root.tag)
                                    or _tag(elem.tag) not in ("page", "siteinfo")):
                                raise DumpDecodeError("unexpected mediawiki root child")
                            if _tag(elem.tag) == "siteinfo":
                                if seen_siteinfo or seen_page:
                                    raise DumpDecodeError("duplicate or misplaced siteinfo")
                                seen_siteinfo = True
                            else:
                                seen_page = True
                                bounded.page_start = bounded.total
                        elif _tag(elem.tag) == "page":
                            raise DumpDecodeError("page outside mediawiki root namespace")
                        continue
                    if _tag(elem.tag) == "page" and depth != 2:
                        raise DumpDecodeError("page outside mediawiki root")
                    if depth == 2 and _tag(elem.tag) == "page":
                        page_id, ns, title, revision_id, timestamp, redirect = _page_identity(elem)
                        if not first <= page_id <= last or page_id <= previous:
                            raise DumpDecodeError("page ID out of shard range or not strictly increasing")
                        previous = page_id
                        pages += 1
                        if ns == namespace:
                            if output_format == "ids":
                                output.write(f"{page_id}\n".encode("ascii"))
                            else:
                                record = {
                                    "schema_version": 1, "project": project,
                                    "generation_id": generation,
                                    "page_id": page_id, "namespace": ns,
                                    "title": title, "revision_id": revision_id,
                                    "timestamp": timestamp, "redirect_target": redirect,
                                }
                                if output_format == "revision-content-jsonl":
                                    wikitext = _revision_wikitext(_one(elem, "revision"))
                                    record["wikitext"] = wikitext
                                    record["wikitext_sha256"] = hashlib.sha256(
                                        wikitext.encode("utf-8")).hexdigest()
                                output.write((json.dumps(
                                    record, ensure_ascii=False, sort_keys=True,
                                    separators=(",", ":")) + "\n").encode("utf-8"))
                            selected += 1
                        elem.clear()
                        root.clear()
                        bounded.page_start = None
                        bounded.non_page_start = bounded.total
                    elif depth == 2 and _tag(elem.tag) == "siteinfo":
                        elem.clear()
                    depth -= 1
                if depth != 0 or root is None or pages == 0:
                    raise DumpDecodeError("empty or incomplete XML member")
            after = os.fstat(handle.fileno())
            if (compressed.size != expected_size
                    or compressed.digest.hexdigest() != expected_sha256
                    or before.st_dev != after.st_dev or before.st_ino != after.st_ino
                    or before.st_size != after.st_size
                    or before.st_mtime_ns != after.st_mtime_ns
                    or before.st_ctime_ns != after.st_ctime_ns):
                raise DumpDecodeError("compressed member changed during decoding")
    except (ET.ParseError, EOFError, OSError, UnicodeError) as exc:
        raise DumpDecodeError("malformed or truncated bzip2/XML member") from exc
    return selected


MAX_EXPECTED_CENSUS_BYTES = 2_000_000_000


def _audit_expected_census(expected_path, expected_sha256, observed_path, selected):
    """Compare a separately supplied, digest-pinned expected census before publish.

    The expected census's publisher, independence, namespace and snapshot cutoff
    MUST be established by the caller. Matching a caller-supplied digest alone
    cannot establish any of those properties.
    """
    if (not isinstance(expected_sha256, str)
            or not re.fullmatch(r"[0-9a-fA-F]{64}", expected_sha256)):
        raise DumpDecodeError("invalid expected census digest")
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0)
    with os.fdopen(os.open(expected_path, flags), "rb") as expected, open(observed_path, "rb") as observed:
        before = os.fstat(expected.fileno())
        observed_meta = os.fstat(observed.fileno())
        if (not stat.S_ISREG(before.st_mode) or before.st_nlink != 1
                or not 0 < before.st_size <= MAX_EXPECTED_CENSUS_BYTES
                or (before.st_dev, before.st_ino) == (observed_meta.st_dev, observed_meta.st_ino)):
            raise DumpDecodeError("unsafe or aliased expected census")
        digest = hashlib.sha256()
        previous = 0
        count = 0
        while True:
            line = expected.readline(21)  # at most 19 decimal digits plus LF
            if not line:
                break
            digest.update(line)
            if (len(line) > 20 or not re.fullmatch(rb"[1-9][0-9]*\n", line)):
                raise DumpDecodeError("invalid expected census record")
            value = int(line[:-1])
            if value > MAX_PAGE_ID or value <= previous:
                raise DumpDecodeError("expected census is not strictly increasing")
            previous = value
            if observed.readline(21) != line:
                raise DumpDecodeError("expected and observed page IDs differ")
            count += 1
        if count != selected or observed.read(1):
            raise DumpDecodeError("expected and observed census lengths differ")
        after = os.fstat(expected.fileno())
        if (digest.hexdigest() != expected_sha256.lower()
                or (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns,
                    before.st_ctime_ns, before.st_nlink)
                != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns,
                    after.st_ctime_ns, after.st_nlink)):
            raise DumpDecodeError("expected census digest mismatch or file changed")


def stage_observed_census(manifest, staging, output_dir, filename, namespace=0,
                          expected_census=None, expected_sha256=None, output_format="ids"):
    """Verify all staged digests, decode all shards, then publish atomically."""
    if output_format not in ("ids", "revision-jsonl", "revision-content-jsonl"):
        raise DumpDecodeError("invalid diagnostic output format")
    if output_format != "ids" and (expected_census is not None or expected_sha256 is not None):
        raise DumpDecodeError("expected census comparison requires ID format")
    if (expected_census is None) != (expected_sha256 is None):
        raise DumpDecodeError("expected census and SHA-256 must be supplied together")
    if type(namespace) is not int or not 0 <= namespace <= MAX_NAMESPACE_ID:
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
    metadata = {}
    for item in manifest["files"]:
        match = NAME.fullmatch(item["name"])
        if not match or match["project"] != manifest["project"] or match["date"] != manifest["generation_id"]:
            raise DumpDecodeError("unrecognized current-content member name")
        if len(match["first"]) > 19 or len(match["last"]) > 19:
            raise DumpDecodeError("invalid declared shard range")
        first, last = int(match["first"]), int(match["last"])
        if not 0 < first <= last <= MAX_PAGE_ID:
            raise DumpDecodeError("invalid declared shard range")
        shards.append((first, last, item["name"]))
        metadata[item["name"]] = item
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
                item = metadata[name]
                selected += decode_member(Path(staging) / name, first, last, sink, namespace,
                                          item["sha256"], item["bytes"], output_format,
                                          manifest["project"], manifest["generation_id"])
            if not selected:
                raise DumpDecodeError("selected namespace has no pages")
            sink.flush()
            os.fsync(sink.fileno())
        if expected_census is not None:
            _audit_expected_census(expected_census, expected_sha256, pending, selected)
        if output_format != "ids":
            try:
                audited, _ = audit_revision_jsonl(
                    pending, manifest["project"], manifest["generation_id"],
                    namespace, output_format == "revision-content-jsonl")
            except RevisionAuditError as exc:
                raise DumpDecodeError("revision record self-audit failed") from exc
            if audited != selected:
                raise DumpDecodeError("revision record self-audit count mismatch")
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
    parser.add_argument("--expected-census", type=Path)
    parser.add_argument("--expected-sha256")
    parser.add_argument("--format", choices=("ids", "revision-jsonl", "revision-content-jsonl"), default="ids")
    args = parser.parse_args()
    result, count = stage_observed_census(load_source_manifest(args.manifest), args.staging, args.output_dir, args.filename, args.namespace,
                                          args.expected_census, args.expected_sha256,
                                          args.format)
    status = "digest-pinned expected census matched; its authority/independence NOT established" if args.expected_census else "publisher authenticity and completeness NOT established"
    print(f"Observed {count} page IDs in {result}; {status}")


if __name__ == "__main__":
    main()
