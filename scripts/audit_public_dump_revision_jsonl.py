#!/usr/bin/env python3
"""Audit diagnostic revision JSONL before downstream use.

This validates locally generated metadata and optional XML-decoded wikitext.
It does NOT authenticate Wikimedia publication, completeness or rendering.
"""
import argparse
from datetime import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import unicodedata

MAX_FILE_BYTES = 2_000_000_000
MAX_RECORD_BYTES = 128 * 1024 * 1024
MAX_TEXT_BYTES = 32 * 1024 * 1024
MAX_RECORDS = 100_000_000
MAX_ID = 2**63 - 1
BASE_FIELDS = frozenset((
    "schema_version", "project", "generation_id", "page_id", "namespace",
    "title", "revision_id", "timestamp", "redirect_target",
))
CONTENT_FIELDS = frozenset(("wikitext", "wikitext_sha256"))


class RevisionAuditError(ValueError):
    """Unsafe, malformed or inconsistent diagnostic record."""


def _unique_pairs(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise RevisionAuditError("duplicate JSON field")
        result[key] = value
    return result


def _reject_constant(value):
    raise RevisionAuditError("nonstandard JSON number")


def _valid_title(value):
    return (isinstance(value, str) and value
            and value == value.strip()
            and len(value.encode("utf-8")) <= 4096
            and not any(unicodedata.category(c) == "Cc" for c in value))


def _valid_timestamp(value):
    if not isinstance(value, str) or not re.fullmatch(
            r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z", value):
        return False
    try:
        datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ")
    except ValueError:
        return False
    return True


def _audit_record(line, project, generation, namespace, content, previous):
    try:
        value = json.loads(
            line.decode("utf-8"), object_pairs_hook=_unique_pairs,
            parse_constant=_reject_constant)
    except (ValueError, UnicodeError, RecursionError) as exc:
        raise RevisionAuditError("invalid diagnostic JSON record") from exc
    fields = BASE_FIELDS | CONTENT_FIELDS if content else BASE_FIELDS
    if not isinstance(value, dict) or value.keys() != fields:
        raise RevisionAuditError("unexpected diagnostic record schema")
    if (type(value["schema_version"]) is not int or value["schema_version"] != 1
            or value["project"] != project or value["generation_id"] != generation
            or type(value["namespace"]) is not int or value["namespace"] != namespace):
        raise RevisionAuditError("wrong diagnostic provenance or namespace")
    page_id, revision_id = value["page_id"], value["revision_id"]
    if (type(page_id) is not int or not previous < page_id <= MAX_ID
            or type(revision_id) is not int or not 0 < revision_id <= MAX_ID):
        raise RevisionAuditError("invalid or unordered page/revision ID")
    if (not _valid_title(value["title"])
            or not _valid_timestamp(value["timestamp"])
            or value["timestamp"][:10] > generation):
        raise RevisionAuditError("invalid title or revision timestamp")
    redirect = value["redirect_target"]
    if redirect is not None and not _valid_title(redirect):
        raise RevisionAuditError("invalid redirect target")
    if content:
        text = value["wikitext"]
        digest = value["wikitext_sha256"]
        if not isinstance(text, str):
            raise RevisionAuditError("invalid wikitext")
        raw = text.encode("utf-8")
        if len(raw) > MAX_TEXT_BYTES:
            raise RevisionAuditError("oversized decoded wikitext")
        if (not isinstance(digest, str)
                or not re.fullmatch(r"[0-9a-f]{64}", digest)
                or hashlib.sha256(raw).hexdigest() != digest):
            raise RevisionAuditError("decoded wikitext digest mismatch")
    return page_id


def audit_revision_jsonl(path, project, generation, namespace, content=False):
    """Stream-validate one immutable local diagnostic export; return count and SHA.

    The returned SHA identifies observed bytes; it is not an independent pin.
    The caller must separately establish source authority and completeness.
    """
    if (not isinstance(project, str) or not 0 < len(project) <= 255
            or not re.fullmatch(r"[a-z0-9_]+", project)
            or not isinstance(generation, str)
            or not re.fullmatch(r"\d{4}-\d{2}-\d{2}", generation)
            or type(namespace) is not int or not 0 <= namespace <= 2**31 - 1):
        raise RevisionAuditError("invalid expected diagnostic scope")
    try:
        datetime.strptime(generation, "%Y-%m-%d")
    except ValueError as exc:
        raise RevisionAuditError("invalid generation date") from exc
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0)
    try:
        with os.fdopen(os.open(path, flags), "rb") as handle:
            before = os.fstat(handle.fileno())
            if (not stat.S_ISREG(before.st_mode) or before.st_nlink != 1
                    or not 0 < before.st_size <= MAX_FILE_BYTES):
                raise RevisionAuditError("unsafe or oversized diagnostic file")
            digest = hashlib.sha256()
            previous = 0
            count = 0
            while True:
                line = handle.readline(MAX_RECORD_BYTES + 1)
                if not line:
                    break
                if (len(line) > MAX_RECORD_BYTES or not line.endswith(b"\n")
                        or b"\r" in line):
                    raise RevisionAuditError("oversized or unterminated JSONL record")
                digest.update(line)
                previous = _audit_record(
                    line, project, generation, namespace, content, previous)
                count += 1
                if count > MAX_RECORDS:
                    raise RevisionAuditError("too many diagnostic records")
            after = os.fstat(handle.fileno())
            identity = lambda s: (
                s.st_dev, s.st_ino, s.st_size, s.st_mtime_ns, s.st_ctime_ns, s.st_nlink)
            if count == 0 or identity(before) != identity(after):
                raise RevisionAuditError("empty or changed diagnostic file")
            return count, digest.hexdigest()
    except OSError as exc:
        raise RevisionAuditError("unsafe or unreadable diagnostic file") from exc


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("path", type=Path)
    parser.add_argument("--project", required=True)
    parser.add_argument("--generation", required=True)
    parser.add_argument("--namespace", type=int, default=0)
    parser.add_argument("--content", action="store_true",
                        help="require revision-content-jsonl records with verified text digests")
    args = parser.parse_args()
    count, digest = audit_revision_jsonl(
        args.path, args.project, args.generation, args.namespace, args.content)
    print(f"Audited {count} diagnostic revision records (SHA-256 {digest}); "
          "upstream authority and completeness NOT established")


if __name__ == "__main__":
    main()
