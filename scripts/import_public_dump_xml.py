#!/usr/bin/env python3
"""Bounded preliminary importer for verified public-dump XML / XML.bz2 members.

Emits revision-scoped raw wikitext NDJSON, NOT rendered HTML or installable
canonical articles. A separate Parsoid/MediaWiki-compatible renderer is still
required before complete offline Wikipedia reader acceptance.
"""
import argparse
import bz2
import hashlib
import json
import os
from pathlib import Path
import tempfile
from xml.parsers import expat

from verify_source_staging import verify_source_bytes

CHUNK = 65536
FIELDS = {
    ("mediawiki", "page", "title"): "title",
    ("mediawiki", "page", "ns"): "namespace",
    ("mediawiki", "page", "id"): "page_id",
    ("mediawiki", "page", "revision", "id"): "revision_id",
    ("mediawiki", "page", "revision", "timestamp"): "timestamp",
    ("mediawiki", "page", "revision", "text"): "wikitext",
    ("mediawiki", "page", "revision", "model"): "model",
    ("mediawiki", "page", "revision", "format"): "format",
    ("mediawiki", "page", "revision", "slots", "slot", "text"): "slot_wikitext",
    ("mediawiki", "page", "revision", "slots", "slot", "model"): "slot_model",
    ("mediawiki", "page", "revision", "slots", "slot", "format"): "slot_format",
}


class DumpImportError(ValueError):
    pass


def parse_xml(stream, emit, *, project, generation, max_pages=1000000,
              max_page_bytes=8 * 1024 * 1024):
    """Streaming parser with bounded page text and explicit revision identity."""
    if not isinstance(project, str) or not project or not generation:
        raise DumpImportError("missing source provenance")
    seen = set()
    path = []
    page = None
    field = None
    chunks = []
    page_bytes = 0
    revisions = 0
    main_slots = 0
    total = 0
    parser = expat.ParserCreate(namespace_separator="}")

    def reject_dtd(*_args):
        raise DumpImportError("DOCTYPE and entity declarations are prohibited")

    def local(tag):
        return tag.rsplit("}", 1)[-1]

    def start(tag, attrs):
        nonlocal page, field, chunks, page_bytes, revisions, main_slots
        element = local(tag)
        path.append(element)
        if len(path) > 32:
            raise DumpImportError("XML nesting depth exceeded")
        if len(path) == 1 and element != "mediawiki":
            raise DumpImportError("not a MediaWiki XML dump")
        if path == ["mediawiki", "page"]:
            page = {}
            revisions = 0
            main_slots = 0
            page_bytes = 0
        elif path == ["mediawiki", "page", "revision"]:
            revisions += 1
            if revisions > 1:
                raise DumpImportError("multiple revisions in current-content dump")
        elif path == ["mediawiki", "page", "revision", "slots", "slot"]:
            if attrs.get("role") != "main":
                raise DumpImportError("unsupported non-main MediaWiki content slot")
            main_slots += 1
            if main_slots > 1:
                raise DumpImportError("duplicate main content slot")
        elif path == ["mediawiki", "page", "redirect"]:
            if page is not None:
                page["redirect_title"] = attrs.get("title")
                if not page["redirect_title"]:
                    raise DumpImportError("redirect has no target title")
        elif tuple(path) in FIELDS:
            if page is None or field is not None:
                raise DumpImportError("invalid nested field")
            field = FIELDS[tuple(path)]
            if field in page:
                raise DumpImportError("duplicate page field")
            chunks = []
        elif field is not None:
            raise DumpImportError("nested markup inside scalar dump field")

    def data(value):
        nonlocal page_bytes
        if page is None:
            return
        page_bytes += len(value.encode("utf-8"))
        if page_bytes > max_page_bytes:
            raise DumpImportError("page exceeded import byte budget")
        if field is not None:
            chunks.append(value)

    def end(tag):
        nonlocal page, field, chunks, total
        if not path or path[-1] != local(tag):
            raise DumpImportError("malformed XML nesting")
        if field is not None and tuple(path) in FIELDS:
            page[field] = "".join(chunks)
            field = None
            chunks = []
        if path == ["mediawiki", "page"]:
            if page is None or revisions != 1:
                raise DumpImportError("missing or ambiguous page revision")
            try:
                page_id = int(page["page_id"])
                revision_id = int(page["revision_id"])
                namespace = int(page["namespace"])
                timestamp = page["timestamp"]
                if "wikitext" in page and "slot_wikitext" in page:
                    raise DumpImportError("ambiguous direct and slot text")
                wikitext = page.get("slot_wikitext", page.get("wikitext"))
                model = page.get("slot_model", page.get("model", "wikitext"))
                content_format = page.get("slot_format", page.get("format", "text/x-wiki"))
                title = page["title"]
            except DumpImportError:
                raise
            except (KeyError, ValueError, TypeError) as error:
                raise DumpImportError("missing or invalid page metadata") from error
            if model != "wikitext" or content_format != "text/x-wiki":
                raise DumpImportError("unsupported content model or format")
            if page_id <= 0 or revision_id <= 0 or not title.strip() or not timestamp or not wikitext:
                raise DumpImportError("invalid or empty current-content record")
            if page_id in seen:
                raise DumpImportError("duplicate page ID in dump member")
            seen.add(page_id)
            total += 1
            if total > max_pages:
                raise DumpImportError("page count budget exceeded")
            emit({
                "project": project, "generation_id": generation,
                "page_id": page_id, "namespace": namespace, "title": title,
                "revision_id": revision_id, "timestamp": timestamp,
                "wikitext_sha256": hashlib.sha256(wikitext.encode("utf-8")).hexdigest(),
                "wikitext": wikitext, "redirect_title": page.get("redirect_title"),
            })
            page = None
        path.pop()

    parser.StartElementHandler = start
    parser.EndElementHandler = end
    parser.CharacterDataHandler = data
    parser.StartDoctypeDeclHandler = reject_dtd
    parser.EntityDeclHandler = reject_dtd
    parser.ExternalEntityRefHandler = lambda *_: 0
    parser.SetParamEntityParsing(expat.XML_PARAM_ENTITY_PARSING_NEVER)
    try:
        while True:
            block = stream.read(CHUNK)
            if not block:
                break
            parser.Parse(block, False)
        parser.Parse(b"", True)
    except (expat.ExpatError, EOFError, OSError) as error:
        raise DumpImportError("invalid or truncated compressed MediaWiki XML") from error
    if total == 0:
        raise DumpImportError("empty MediaWiki XML dump")
    return total


def import_verified_members(manifest, staging, members, output, *, max_pages=1000000):
    """Atomically combine explicitly selected, verified, non-overlapping XML shards.

    Caller must select shards from one dump product. A completed generation
    may contain *different* dump products with overlapping pages; silently
    importing every XML file would duplicate or mix source semantics.
    """
    verify_source_bytes(manifest, staging)
    if (isinstance(members, (str, bytes)) or not isinstance(members, (list, tuple))
            or not members or type(max_pages) is not int or max_pages <= 0):
        raise DumpImportError("invalid XML member selection or page budget")
    allowed = {entry["name"] for entry in manifest["files"]}
    if len(set(members)) != len(members):
        raise DumpImportError("duplicate XML member selection")
    for member in members:
        if (not isinstance(member, str) or member not in allowed
                or not member.endswith((".xml", ".xml.bz2"))):
            raise DumpImportError("member is not a verified XML dump")

    output = Path(output)
    if output.parent.is_symlink() or not output.parent.is_dir():
        raise DumpImportError("unsafe output directory")
    seen_pages = set()
    total = 0
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", dir=output.parent,
            prefix=".raw-dump.", suffix=".tmp", delete=False
        ) as writer:
            temporary = Path(writer.name)

            def emit(record):
                nonlocal total
                identity = (record["project"], record["page_id"])
                if identity in seen_pages:
                    raise DumpImportError("duplicate page ID across dump members")
                if total >= max_pages:
                    raise DumpImportError("combined page count budget exceeded")
                seen_pages.add(identity)
                total += 1
                writer.write(json.dumps(record, ensure_ascii=False, sort_keys=True) + "\n")

            for member in members:
                source = Path(staging) / member
                stream_open = bz2.open if member.endswith(".bz2") else open
                with stream_open(source, "rb") as stream:
                    parse_xml(stream, emit, project=manifest["project"],
                              generation=manifest["generation_id"],
                              max_pages=max_pages)
            writer.flush()
            os.fsync(writer.fileno())
        try:
            os.link(temporary, output)
        except FileExistsError as error:
            raise DumpImportError("raw import output already exists") from error
        directory_fd = os.open(output.parent, os.O_RDONLY)
        try:
            os.fsync(directory_fd)
        finally:
            os.close(directory_fd)
        return total
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def import_verified_member(manifest, staging, member, output):
    """Backward-compatible single-member raw import."""
    return import_verified_members(manifest, staging, [member], output)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source_manifest", type=Path)
    parser.add_argument("staging_directory", type=Path)
    parser.add_argument("dump_member")
    parser.add_argument("--additional-member", action="append", default=[],
                        help="Additional non-overlapping XML shard (repeatable)")
    parser.add_argument("raw_ndjson_output", type=Path)
    args = parser.parse_args()
    manifest = json.loads(args.source_manifest.read_text(encoding="utf-8"))
    try:
        count = import_verified_members(
            manifest, args.staging_directory,
            [args.dump_member, *args.additional_member], args.raw_ndjson_output
        )
    except (DumpImportError, OSError, ValueError) as error:
        parser.exit(2, f"Raw XML import failed: {error}\n")
    print(f"Imported {count} raw revision-scoped wikitext page(s). "
          "No HTML rendering, canonical article records, or installation performed.")


if __name__ == "__main__":
    main()
