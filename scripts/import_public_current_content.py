#!/usr/bin/env python3
"""Verified public current-content XML/bzip2 reader; never renders wikitext."""
import bz2
from dataclasses import dataclass
import hashlib
import io
import os
from pathlib import Path
import stat
import xml.etree.ElementTree as ET


class PublicDumpError(ValueError):
    pass


@dataclass(frozen=True)
class PublicDumpPage:
    page_id: int
    revision_id: int
    title: str
    namespace: int
    timestamp: str
    wikitext: str
    redirect_title: str | None


def _child(element, name):
    matches = [item for item in element if item.tag.rsplit("}", 1)[-1] == name]
    if len(matches) > 1:
        raise PublicDumpError(f"duplicate {name}")
    return matches[0] if matches else None


def _required(element, name):
    child = _child(element, name)
    if child is None or not child.text or not child.text.strip():
        raise PublicDumpError(f"missing {name}")
    return child.text


def _page(element):
    try:
        page_id = int(_required(element, "id"))
        namespace = int(_required(element, "ns"))
        revisions = [child for child in element if child.tag.rsplit("}", 1)[-1] == "revision"]
        if len(revisions) != 1:
            raise PublicDumpError("expected one current revision")
        revision = revisions[0]
        revision_id = int(_required(revision, "id"))
    except PublicDumpError:
        raise
    except ValueError as error:
        raise PublicDumpError("invalid page or revision identifier") from error
    if page_id <= 0 or revision_id <= 0:
        raise PublicDumpError("nonpositive page/revision identifier")
    title = _required(element, "title")
    timestamp = _required(revision, "timestamp")
    text = _child(revision, "text")
    if text is None or text.get("deleted") is not None:
        raise PublicDumpError("missing or deleted revision text")
    redirect = _child(element, "redirect")
    redirect_title = redirect.get("title") if redirect is not None else None
    if redirect is not None and not redirect_title:
        raise PublicDumpError("redirect without target")
    wikitext = text.text or ""
    if len(wikitext) > 16 * 1024 * 1024:
        raise PublicDumpError("oversized page wikitext")
    return PublicDumpPage(
        page_id, revision_id, title, namespace, timestamp,
        wikitext, redirect_title,
    )


def iter_verified_pages(path, expected_sha256):
    """Yield only from a checksum-verified regular file; caller stages atomically.

    Exhaust the iterator and validate the complete XML before activating output.
    A failed later page must discard the entire staged generation.
    """
    if not isinstance(expected_sha256, str) or len(expected_sha256) != 64:
        raise PublicDumpError("invalid expected SHA-256")
    path = Path(path)
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(path, flags)
    with os.fdopen(descriptor, "rb") as source:
        info = os.fstat(source.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
            raise PublicDumpError("unsafe public dump file")
        digest = hashlib.sha256()
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
        if digest.hexdigest() != expected_sha256.lower():
            raise PublicDumpError("public dump checksum mismatch")
        source.seek(0)
        with bz2.BZ2File(source) as decompressed:
            buffered = io.BufferedReader(decompressed)
            try:
                header = buffered.peek(65536)[:65536]
            except (EOFError, OSError) as error:
                raise PublicDumpError("invalid bzip2 stream") from error
            if b"<!DOCTYPE" in header.upper() or b"<mediawiki" not in header:
                raise PublicDumpError("unsafe or unrecognized XML prolog")
            root = None
            seen = set()
            try:
                for event, element in ET.iterparse(buffered, events=("start", "end")):
                    if root is None:
                        root = element
                        if element.tag.rsplit("}", 1)[-1] != "mediawiki":
                            raise PublicDumpError("not a MediaWiki XML export")
                    if event == "end" and element.tag.rsplit("}", 1)[-1] == "page":
                        page = _page(element)
                        if page.page_id in seen:
                            raise PublicDumpError("duplicate page identifier")
                        seen.add(page.page_id)
                        yield page
                        root.clear()
            except (ET.ParseError, EOFError, OSError) as error:
                raise PublicDumpError("invalid or truncated XML/bzip2 dump") from error
