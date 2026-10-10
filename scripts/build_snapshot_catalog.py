#!/usr/bin/env python3
"""Build and query a provisional SQLite index for immutable FWREC001 shards.

Input NDJSON is a caller-supplied index proposal, NOT authenticated source data.
Each frame is verified against its actual shard bytes. Canonical PageRecord
identity verification and full-snapshot activation remain STORE-001 work.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import sqlite3
import struct
import sys

MAX_FRAME = 32 * 1024 * 1024 + 40
SCHEMA = """
PRAGMA foreign_keys=ON;
CREATE TABLE records (
 project TEXT NOT NULL, page_id INTEGER NOT NULL, title TEXT NOT NULL,
 revision_id INTEGER, wikidata_id TEXT, shard TEXT NOT NULL,
 byte_offset INTEGER NOT NULL, frame_bytes INTEGER NOT NULL,
 frame_sha256 TEXT NOT NULL, PRIMARY KEY(project,page_id)
);
CREATE TABLE titles (
 project TEXT NOT NULL, normalized_title TEXT NOT NULL, page_id INTEGER NOT NULL,
 PRIMARY KEY(project,normalized_title),
 FOREIGN KEY(project,page_id) REFERENCES records(project,page_id)
);
CREATE INDEX by_wikidata ON records(project,wikidata_id);
"""

class CatalogError(ValueError):
    pass

def title_key(title):
    if not isinstance(title, str) or not title.strip():
        raise CatalogError("invalid title")
    return title.strip().replace("_", " ").casefold()

def integer(value, *, positive=True):
    if type(value) is not int or value < (1 if positive else 0) or value >= 2**63:
        raise CatalogError("invalid nonnegative SQLite integer")
    return value

def shard_path(root, name):
    if not isinstance(name, str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,254}", name) or name in {".", ".."}:
        raise CatalogError("unsafe shard name")
    path = root / name
    if path.is_symlink() or not path.is_file() or path.stat().st_nlink != 1:
        raise CatalogError("unsafe shard file")
    return path

def read_frame(root, name, offset, size, expected=None):
    path = shard_path(root, name)
    offset = integer(offset, positive=False)
    size = integer(size)
    if not 40 <= size <= MAX_FRAME:
        raise CatalogError("invalid frame size")
    with path.open("rb") as stream:
        before = os.fstat(stream.fileno())
        if before.st_ino != path.stat().st_ino or before.st_dev != path.stat().st_dev:
            raise CatalogError("shard replaced")
        if offset + size > before.st_size:
            raise CatalogError("frame outside shard")
        stream.seek(offset)
        frame = stream.read(size)
        after = os.fstat(stream.fileno())
        if len(frame) != size or (before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (after.st_size, after.st_mtime_ns, after.st_ctime_ns):
            raise CatalogError("shard changed during read")
    if struct.unpack("<Q", frame[:8])[0] != size - 8 or frame[8:16] != b"FWREC001":
        raise CatalogError("invalid record frame")
    raw_len, compressed_len = struct.unpack("<QQ", frame[24:40])
    if not 0 < raw_len <= 32 * 1024 * 1024 or compressed_len != size - 40 or compressed_len > 32 * 1024 * 1024:
        raise CatalogError("invalid record lengths")
    digest = hashlib.sha256(frame).hexdigest()
    if expected is not None and digest != expected:
        raise CatalogError("frame digest mismatch")
    return frame, digest

def validate_entry(entry):
    if not isinstance(entry, dict):
        raise CatalogError("entry must be an object")
    project = entry.get("project")
    if not isinstance(project, str) or not re.fullmatch(r"[a-z0-9_]+", project):
        raise CatalogError("invalid project")
    page_id = integer(entry.get("page_id"))
    title = entry.get("title")
    title_key(title)
    revision = entry.get("revision_id")
    if revision is not None:
        integer(revision)
    wikidata = entry.get("wikidata_id")
    if wikidata is not None and (not isinstance(wikidata, str) or not re.fullmatch(r"Q[1-9][0-9]*", wikidata)):
        raise CatalogError("invalid Wikidata ID")
    aliases = entry.get("aliases", [])
    if not isinstance(aliases, list):
        raise CatalogError("invalid aliases")
    names = {title_key(title)}
    for alias in aliases:
        names.add(title_key(alias))
    return project, page_id, title, revision, wikidata, sorted(names)

def build(manifest, root, destination):
    if destination.exists() or destination.is_symlink():
        raise CatalogError("refusing to overwrite catalog")
    if not root.is_dir() or root.is_symlink():
        raise CatalogError("unsafe shard root")
    temporary = destination.with_name(destination.name + ".partial." + str(os.getpid()))
    if temporary.exists() or temporary.is_symlink():
        raise CatalogError("temporary catalog already exists")
    try:
        conn = sqlite3.connect(temporary)
        try:
            conn.executescript(SCHEMA)
            with conn:
                with manifest.open("r", encoding="utf-8") as source:
                    for line_number, line in enumerate(source, 1):
                        if len(line) > 65536:
                            raise CatalogError(f"oversized descriptor at line {line_number}")
                        entry = json.loads(line)
                        project, page_id, title, revision, wikidata, names = validate_entry(entry)
                        _, digest = read_frame(root, entry.get("shard"), entry.get("offset"), entry.get("frame_bytes"))
                        conn.execute("INSERT INTO records VALUES (?,?,?,?,?,?,?,?,?)", (
                            project, page_id, title, revision, wikidata, entry["shard"],
                            entry["offset"], entry["frame_bytes"], digest,
                        ))
                        for name in names:
                            conn.execute("INSERT INTO titles VALUES (?,?,?)", (project, name, page_id))
            conn.execute("PRAGMA wal_checkpoint(TRUNCATE)")
        finally:
            conn.close()
        # Do not publish a partial index on any validation or constraint failure.
        os.link(temporary, destination)
    finally:
        temporary.unlink(missing_ok=True)

def lookup(catalog, root, project, title):
    with sqlite3.connect(f"file:{catalog}?mode=ro", uri=True) as conn:
        conn.execute("PRAGMA query_only=ON")
        row = conn.execute("""
          SELECT r.page_id,r.revision_id,r.shard,r.byte_offset,r.frame_bytes,r.frame_sha256
          FROM records r JOIN titles t USING(project,page_id)
          WHERE t.project=? AND t.normalized_title=?
        """, (project, title_key(title))).fetchone()
    if row is None:
        return None
    page_id, revision, shard, offset, size, digest = row
    frame, _ = read_frame(root, shard, offset, size, digest)
    return {"project": project, "page_id": page_id, "revision_id": revision,
            "frame_sha256": digest, "frame": frame}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    create = sub.add_parser("build")
    create.add_argument("manifest", type=Path)
    create.add_argument("shards", type=Path)
    create.add_argument("catalog", type=Path)
    query = sub.add_parser("lookup")
    query.add_argument("catalog", type=Path)
    query.add_argument("shards", type=Path)
    query.add_argument("project")
    query.add_argument("title")
    args = parser.parse_args()
    try:
        if args.command == "build":
            build(args.manifest, args.shards, args.catalog)
        else:
            result = lookup(args.catalog, args.shards, args.project, args.title)
            print(json.dumps(None if result is None else {
                key: value for key, value in result.items() if key != "frame"
            }, sort_keys=True))
    except (CatalogError, OSError, sqlite3.Error, json.JSONDecodeError) as error:
        parser.exit(1, f"catalog: {error}\n")

if __name__ == "__main__":
    main()
