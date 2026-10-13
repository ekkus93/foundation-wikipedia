#!/usr/bin/env python3
"""Stage one complete caller-authenticated current-content export as JSONL.

The supplied SHA256SUMS must be authenticated independently against a trusted
Wikimedia publication. A local checksum file and matching bytes do NOT prove
publisher identity, completeness of the real export, or offline article fidelity.
The result is raw, unrendered wikitext, not an installable pack.
"""
import argparse
from contextlib import closing
from datetime import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import sqlite3
import tempfile

from extract_public_dump_pages import (DumpExtractionError, _source_identity,
                                       _verified_source, extract_current_dump)

MAX_INVENTORY_BYTES = 32 * 1024 * 1024
MAX_MEMBERS = 100_000
MAX_MEMBER_BYTES = 1024 * 1024 * 1024
MAX_TOTAL_BYTES = 1 << 44
_RESERVED = {"CON", "PRN", "AUX", "NUL"}


def _safe_member_path(path):
    parts = path.split("/")
    if not path or len(path) > 4096 or len(parts) > 64:
        return False
    for part in parts:
        base = part.split(".")[0].upper()
        if (not part or len(part) > 255 or part in (".", "..")
                or part.endswith(".") or base in _RESERVED
                or (len(base) == 4 and base[:3] in ("COM", "LPT")
                    and base[3] in "123456789")
                or not re.fullmatch(r"[A-Za-z0-9_.-]+", part)):
            return False
    return True


def _inventory(path, project, date):
    if not re.fullmatch(r"[a-z0-9_]{1,64}", project):
        raise DumpExtractionError("invalid export project")
    if not re.fullmatch(r"[0-9]{4}-[0-9]{2}-01", date):
        raise DumpExtractionError("invalid export date")
    try:
        datetime.strptime(date, "%Y-%m-%d")
    except ValueError:
        raise DumpExtractionError("invalid export date") from None
    if path.is_symlink():
        raise DumpExtractionError("symlinked checksum inventory")
    fd = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    with os.fdopen(fd, "rb") as reader:
        meta = os.fstat(reader.fileno())
        if (not stat.S_ISREG(meta.st_mode) or meta.st_nlink != 1
                or not 0 < meta.st_size <= MAX_INVENTORY_BYTES):
            raise DumpExtractionError("unsafe or oversized checksum inventory")
        text = reader.read(MAX_INVENTORY_BYTES + 1)
        # Reading can update atime on some filesystems. Compare stable inode
        # identity and mutation metadata, not access time.
        if (_source_identity(os.fstat(reader.fileno())) != _source_identity(meta)
                or len(text) != meta.st_size):
            raise DumpExtractionError("checksum inventory changed during read")
    if not text or len(text) > MAX_INVENTORY_BYTES:
        raise DumpExtractionError("empty or oversized checksum inventory")
    try:
        records = text.decode("ascii").splitlines()
    except UnicodeDecodeError:
        raise DumpExtractionError("non-ASCII checksum inventory") from None
    if not records or len(records) > MAX_MEMBERS:
        raise DumpExtractionError("invalid checksum inventory length")
    parsed = []
    seen = set()
    # Support both the new content-current export and public numbered
    # pages-meta-current partitions without mixing their generations.
    current_prefix = re.compile(
        rf"{re.escape(project)}-{re.escape(date)}-p([0-9]+)p([0-9]+)\.xml\.bz2"
    )
    legacy_prefix = re.compile(
        rf"{re.escape(project)}-{date.replace('-', '')}-pages-meta-current"
        r"([1-9][0-9]*)\.xml-p([0-9]+)p([0-9]+)\.bz2"
    )
    formats = set()
    numbered_shards = set()
    for line in records:
        match = re.fullmatch(r"([0-9A-Fa-f]{64}) [ *](.+)", line)
        if match is None:
            raise DumpExtractionError("invalid checksum inventory line")
        digest, relative = match.groups()
        if not _safe_member_path(relative) or relative.lower() in seen:
            raise DumpExtractionError("unsafe or duplicate shard path")
        seen.add(relative.lower())
        basename = relative.rsplit("/", 1)[-1]
        current = current_prefix.fullmatch(basename)
        legacy = legacy_prefix.fullmatch(basename)
        if current is None and legacy is None:
            raise DumpExtractionError("unexpected export member identity")
        formats.add("current-content" if current is not None else "pages-meta-current")
        if legacy is not None:
            number = int(legacy.group(1))
            if number in numbered_shards:
                raise DumpExtractionError("duplicate numbered shard identity")
            numbered_shards.add(number)
        range_fields = current.groups() if current is not None else legacy.groups()[1:]
        first, last = (int(value) for value in range_fields)
        if not 0 < first <= last <= 2**63 - 1:
            raise DumpExtractionError("invalid shard page-ID partition")
        parsed.append((first, last, relative, digest.lower()))
    if len(formats) != 1:
        raise DumpExtractionError("mixed current-content export formats")
    if numbered_shards and numbered_shards != set(range(1, len(parsed) + 1)):
        raise DumpExtractionError("incomplete numbered shard sequence")
    parsed.sort()
    for previous, current in zip(parsed, parsed[1:]):
        if previous[1] >= current[0]:
            raise DumpExtractionError("overlapping shard page-ID partitions")
    return parsed


def _private_shard(root, relative):
    path = root
    parts = relative.split("/")
    for index, part in enumerate(parts):
        path = path / part
        meta = path.lstat()
        if index == len(parts) - 1:
            if not stat.S_ISREG(meta.st_mode) or meta.st_nlink != 1:
                raise DumpExtractionError("unsafe staged shard")
        elif not stat.S_ISDIR(meta.st_mode):
            raise DumpExtractionError("unsafe staged shard directory")
    return path



def _verify_exact_staged_members(root, records, checksum_file):
    """Reject undeclared files, directories, links and leftover temp shards.

    This is a fail-closed preflight only. The caller must keep the private
    staging tree free from concurrent modifications during extraction.
    """
    expected_files = {name for _, _, name, _ in records}
    # A staged publication produced by stage_completed_export includes only
    # these two fixed metadata files alongside the digest-declared shards.
    # Neither file proves publisher authenticity or offline-readiness.
    if checksum_file.parent == root and checksum_file.name == "SHA256SUMS":
        expected_files.update(("SHA256SUMS", "PUBLICATION.json"))
    expected_directories = set()
    for relative in expected_files:
        parts = relative.split("/")
        for count in range(1, len(parts)):
            expected_directories.add("/".join(parts[:count]))
    try:
        metadata = root.lstat()
        if not stat.S_ISDIR(metadata.st_mode):
            raise DumpExtractionError("unsafe staged export root")
        for ancestor in root.parents:
            if ancestor.is_symlink():
                raise DumpExtractionError("symlinked staging ancestor")
        pending = [(root, "")]
        observed = set()
        while pending:
            directory, prefix = pending.pop()
            for item in directory.iterdir():
                relative = prefix + item.name
                kind = item.lstat().st_mode
                if stat.S_ISREG(kind):
                    if relative not in expected_files:
                        raise DumpExtractionError("undeclared staged export file")
                    observed.add(relative)
                elif stat.S_ISDIR(kind):
                    if relative not in expected_directories:
                        raise DumpExtractionError("undeclared staged export directory")
                    pending.append((item, relative + "/"))
                else:
                    raise DumpExtractionError("unsafe staged export entry")
        if observed != expected_files:
            raise DumpExtractionError("missing declared staged export member")
    except OSError as error:
        raise DumpExtractionError("could not verify exact staged export tree") from error


def _validate_staged_publication_receipt(root, checksum_file, records, project, date,
                                         verified_total_bytes=None):
    """Check local staged metadata consistency, NOT Wikimedia authenticity."""
    if checksum_file.parent != root or checksum_file.name != "SHA256SUMS":
        return
    receipt_path = root / "PUBLICATION.json"
    try:
        fd = os.open(receipt_path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
                     | getattr(os, "O_NONBLOCK", 0))
        with os.fdopen(fd, "rb") as reader:
            info = os.fstat(reader.fileno())
            if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or not 0 < info.st_size <= 65536:
                raise DumpExtractionError("unsafe staged publication receipt")
            payload = reader.read(65537)
            if len(payload) != info.st_size or os.fstat(reader.fileno()) != info:
                raise DumpExtractionError("staged publication receipt changed")
        receipt = json.loads(payload)
    except (OSError, ValueError, TypeError, UnicodeDecodeError) as error:
        raise DumpExtractionError("invalid staged publication receipt") from error
    if not isinstance(receipt, dict) or not isinstance(receipt.get("publication"), dict):
        raise DumpExtractionError("invalid staged publication receipt identity")
    if (type(receipt.get("compressed_bytes")) is not int
            or not 0 < receipt["compressed_bytes"] <= MAX_TOTAL_BYTES):
        raise DumpExtractionError("invalid staged publication compressed byte count")
    if (verified_total_bytes is not None
            and receipt["compressed_bytes"] != verified_total_bytes):
        raise DumpExtractionError("staged publication compressed byte count mismatch")
    publication = receipt["publication"]
    expected_url = (
        "https://dumps.wikimedia.org/other/mediawiki_content_current/"
        f"{project}/{date}/xml/bzip2/SHA256SUMS"
    )
    if (publication.get("project") != project
            or publication.get("date") != date
            or publication.get("checksum_url") != expected_url
            or publication.get("member_count") != len(records)
            or receipt.get("members") != len(records)
            or receipt.get("source") != "Wikimedia mediawiki_content_current; unrendered"
            or receipt.get("status") != "staged-only; NOT offline-ready"):
        raise DumpExtractionError("staged publication identity mismatch")
    # The inventory is already parsed using a bounded, no-symlink reader, and
    # the complete staging tree is exclusive to the caller by contract.
    if publication.get("sha256") != hashlib.sha256(checksum_file.read_bytes()).hexdigest():
        raise DumpExtractionError("staged publication inventory digest mismatch")


def extract_current_export(staging_dir, checksum_file, project, date, output,
                           *, max_member_bytes=MAX_MEMBER_BYTES,
                           max_total_bytes=MAX_TOTAL_BYTES,
                           max_total_pages=10_000_000, max_output_bytes=1 << 40):
    """All declared members must pass before atomically publishing combined JSONL."""
    staging_dir, checksum_file, output = map(Path, (staging_dir, checksum_file, output))
    if (not 0 < max_member_bytes <= MAX_TOTAL_BYTES
            or not 0 < max_total_bytes <= MAX_TOTAL_BYTES
            or not 0 < max_total_pages <= 10_000_000
            or not 0 < max_output_bytes <= MAX_TOTAL_BYTES):
        raise DumpExtractionError("invalid export budget")
    if (not stat.S_ISDIR(staging_dir.lstat().st_mode)
            or not stat.S_ISDIR(output.parent.lstat().st_mode)
            or output.exists() or output.is_symlink()):
        raise DumpExtractionError("unsafe staging/output path or existing output")
    records = _inventory(checksum_file, project, date)
    _verify_exact_staged_members(staging_dir, records, checksum_file)
    _validate_staged_publication_receipt(
        staging_dir, checksum_file, records, project, date,
    )
    paths = []
    total_bytes = 0
    for _, _, name, digest in records:
        path = _private_shard(staging_dir, name)
        source, metadata = _verified_source(path, digest, max_member_bytes)
        source.close()
        total_bytes += metadata.st_size
        if total_bytes > max_total_bytes:
            raise DumpExtractionError("export exceeds aggregate compressed byte budget")
        paths.append((path, digest))
    with tempfile.TemporaryDirectory(prefix=".dump-export.", dir=output.parent) as directory:
        work = Path(directory)
        count = 0
        joined = work / "combined.jsonl"
        # Revision IDs must be unique across the complete export, not just
        # within individual non-overlapping page-ID partitions. Disk-backed
        # uniqueness avoids retaining the entire set in process memory.
        with closing(sqlite3.connect(work / "seen-revisions.sqlite")) as revisions:
            revisions.execute("CREATE TABLE revisions (id INTEGER PRIMARY KEY)")
            revisions.execute("CREATE TABLE titles (namespace INTEGER, title TEXT, PRIMARY KEY(namespace, title))")
            with joined.open("wb") as writer:
                for index, (path, digest) in enumerate(paths):
                    part = work / f"part-{index:06d}.jsonl"
                    remaining = max_total_pages - count
                    if remaining <= 0:
                        raise DumpExtractionError("export exceeds aggregate page budget")
                    count += extract_current_dump(
                        path, digest, project, part,
                        max_compressed_bytes=max_member_bytes, max_pages=remaining,
                    )
                    with part.open("rb") as reader:
                        for line in reader:
                            record = json.loads(line)
                            try:
                                revisions.execute(
                                    "INSERT INTO revisions (id) VALUES (?)",
                                    (record["revision_id"],),
                                )
                            except sqlite3.IntegrityError as error:
                                raise DumpExtractionError(
                                    "duplicate revision ID across export shards"
                                ) from error
                            try:
                                revisions.execute(
                                    "INSERT INTO titles (namespace, title) VALUES (?, ?)",
                                    (record["namespace"], record["title"]),
                                )
                            except sqlite3.IntegrityError as error:
                                raise DumpExtractionError(
                                    "duplicate namespace/title across export pages"
                                ) from error
                            if writer.tell() + len(line) > max_output_bytes:
                                raise DumpExtractionError(
                                    "export exceeds aggregate JSONL byte budget"
                                )
                            writer.write(line)
                    part.unlink()
                writer.flush()
                os.fsync(writer.fileno())
        # No publication if the source changed after an earlier shard was parsed.
        for path, digest in paths:
            source, _ = _verified_source(path, digest, max_member_bytes)
            source.close()
        _verify_exact_staged_members(staging_dir, records, checksum_file)
        # Re-parse the same local SHA256SUMS after extraction: a private tree
        # may have changed while its members were being processed. Never
        # publish records against a different declared source inventory.
        if _inventory(checksum_file, project, date) != records:
            raise DumpExtractionError("checksum inventory changed during extraction")
        _validate_staged_publication_receipt(
            staging_dir, checksum_file, records, project, date,
            verified_total_bytes=total_bytes,
        )
        try:
            os.link(joined, output)
        except FileExistsError:
            raise DumpExtractionError("output already exists") from None
        directory_fd = os.open(output.parent, os.O_RDONLY)
        try:
            os.fsync(directory_fd)
        finally:
            os.close(directory_fd)
        return count


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("private_staging_directory", type=Path)
    parser.add_argument("authenticated_sha256sums", type=Path)
    parser.add_argument("project")
    parser.add_argument("export_date")
    parser.add_argument("output_jsonl", type=Path)
    args = parser.parse_args()
    count = extract_current_export(
        args.private_staging_directory, args.authenticated_sha256sums,
        args.project, args.export_date, args.output_jsonl,
    )
    print(f"Extracted {count} unrendered records; NOT an offline-ready pack.")


if __name__ == "__main__":
    main()
