#!/usr/bin/env python3
"""Stage one complete caller-authenticated current-content export as JSONL.

The supplied SHA256SUMS must be authenticated independently against a trusted
Wikimedia publication. A local checksum file and matching bytes do NOT prove
publisher identity, completeness of the real export, or offline article fidelity.
The result is raw, unrendered wikitext, not an installable pack.
"""
import argparse
from datetime import datetime
import hashlib
import os
from pathlib import Path
import re
import stat
import tempfile

from extract_public_dump_pages import DumpExtractionError, _verified_source, extract_current_dump

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
        if os.fstat(reader.fileno()) != meta:
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
        r"[1-9][0-9]*\.xml-p([0-9]+)p([0-9]+)\.bz2"
    )
    formats = set()
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
        range_match = current if current is not None else legacy
        first, last = (int(value) for value in range_match.groups())
        if not 0 < first <= last <= 2**63 - 1:
            raise DumpExtractionError("invalid shard page-ID partition")
        parsed.append((first, last, relative, digest.lower()))
    if len(formats) != 1:
        raise DumpExtractionError("mixed current-content export formats")
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



def _verify_exact_staged_members(root, records):
    """Reject undeclared files, directories, links and leftover temp shards.

    This is a fail-closed preflight only. The caller must keep the private
    staging tree free from concurrent modifications during extraction.
    """
    expected_files = {name for _, _, name, _ in records}
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
    _verify_exact_staged_members(staging_dir, records)
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
                    for chunk in iter(lambda: reader.read(1024 * 1024), b""):
                        if writer.tell() + len(chunk) > max_output_bytes:
                            raise DumpExtractionError("export exceeds aggregate JSONL byte budget")
                        writer.write(chunk)
                part.unlink()
            writer.flush()
            os.fsync(writer.fileno())
        # No publication if the source changed after an earlier shard was parsed.
        for path, digest in paths:
            source, _ = _verified_source(path, digest, max_member_bytes)
            source.close()
        _verify_exact_staged_members(staging_dir, records)
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
