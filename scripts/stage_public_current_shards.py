#!/usr/bin/env python3
"""Stage checksum-verified official current-content XML shards.

Completed shards survive restart; incomplete transfers persist in private
`.part` files and resume only after strict byte-range response validation.
Caller must authenticate the checksum inventory before using this transport.
"""
import argparse
import hashlib
import os
from pathlib import Path
import re
import stat
from urllib.error import HTTPError, URLError
from urllib.request import Request, build_opener

from discover_public_current_export import NoRedirect, official_url
from extract_public_dump_export import DumpExtractionError, _inventory
from extract_public_dump_pages import _verified_source

CHUNK = 1024 * 1024
CONTENT_RANGE = re.compile(r"bytes ([0-9]+)-([0-9]+)/([0-9]+)")


def _response_length(response):
    length = response.headers.get("Content-Length")
    if length is None:
        return None
    if not length.isdecimal() or int(length) <= 0:
        raise DumpExtractionError("invalid shard Content-Length")
    return int(length)


def _publish_partial(part, target):
    try:
        os.link(part, target, follow_symlinks=False)
    except FileExistsError:
        raise DumpExtractionError("staged shard appeared concurrently") from None
    directory = os.open(target.parent, os.O_RDONLY)
    try:
        os.fsync(directory)
        part.unlink()
        os.fsync(directory)
    finally:
        os.close(directory)


def _download_verified(url, target, expected, opener, member_limit, remaining):
    """Resume one immutable checksum-identified object without using unverified bytes.

    The staging directory must be private and not concurrently writable by
    untrusted processes. Partial files are never considered published content.
    """
    part = target.with_name(target.name + ".part")
    flags = os.O_RDWR | os.O_CREAT | getattr(os, "O_NOFOLLOW", 0)
    try:
        fd = os.open(part, flags, 0o600)
    except OSError as error:
        raise DumpExtractionError("unsafe interrupted shard") from error
    with os.fdopen(fd, "r+b") as writer:
        info = os.fstat(writer.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
            raise DumpExtractionError("unsafe interrupted shard type or hardlink")
        if info.st_size > member_limit or info.st_size > remaining:
            raise DumpExtractionError("interrupted shard exceeds download budget")
        digest = hashlib.sha256()
        offset = 0
        writer.seek(0)
        while chunk := writer.read(CHUNK):
            offset += len(chunk)
            digest.update(chunk)

        # A process may have crashed after fsync and before no-clobber publish.
        if offset and digest.hexdigest() == expected:
            _publish_partial(part, target)
            return offset, "resumed"

        headers = {"User-Agent": "FoundationWikipedia/0.1",
                   "Accept-Encoding": "identity"}
        if offset:
            headers["Range"] = f"bytes={offset}-"
        try:
            with opener.open(Request(url, headers=headers), timeout=30) as response:
                if response.geturl() != url or response.status != (206 if offset else 200):
                    raise DumpExtractionError("unexpected official shard response")
                if response.headers.get("Content-Encoding", "identity").lower() != "identity":
                    raise DumpExtractionError("non-byte-addressable shard encoding")
                length = _response_length(response)
                expected_total = None
                if offset:
                    match = CONTENT_RANGE.fullmatch(
                        response.headers.get("Content-Range", "")
                    )
                    if match is None:
                        raise DumpExtractionError("missing or invalid shard Content-Range")
                    first, last, expected_total = map(int, match.groups())
                    if (first != offset or last < first
                            or last + 1 != expected_total
                            or expected_total > member_limit
                            or expected_total > remaining):
                        raise DumpExtractionError("invalid shard resume range")
                    if length is not None and length != expected_total - offset:
                        raise DumpExtractionError("resumed shard Content-Length mismatch")
                elif length is not None and length > min(member_limit, remaining):
                    raise DumpExtractionError("shard exceeds download budget")

                writer.seek(0, os.SEEK_END)
                size = offset
                transferred = 0
                while chunk := response.read(CHUNK):
                    size += len(chunk)
                    transferred += len(chunk)
                    if size > member_limit or size > remaining:
                        raise DumpExtractionError("shard exceeds download budget")
                    if expected_total is not None and size > expected_total:
                        raise DumpExtractionError("excess resumed shard bytes")
                    if length is not None and transferred > length:
                        raise DumpExtractionError("excess shard response bytes")
                    writer.write(chunk)
                    digest.update(chunk)
                if (length is not None and transferred != length
                        or expected_total is not None and size != expected_total):
                    raise DumpExtractionError("interrupted or truncated shard transfer")
                writer.flush()
                os.fsync(writer.fileno())
        except (HTTPError, URLError, TimeoutError, ConnectionError) as error:
            raise DumpExtractionError("official shard HTTPS failure") from error

        if size == 0 or digest.hexdigest() != expected:
            # Completed but mismatched bytes cannot be trusted as a resume base.
            writer.close()
            part.unlink(missing_ok=True)
            raise DumpExtractionError("shard SHA-256 mismatch")

    _publish_partial(part, target)
    return size, "resumed" if offset else "downloaded"


def stage_verified_members(inventory, project, date, staging_dir, *, opener=None,
                           max_member_bytes=1 << 30, max_total_bytes=1 << 44):
    """Verify every downloaded member before no-overwrite publication."""
    prefix = official_url(project, date).removesuffix("SHA256SUMS")
    if not (0 < max_member_bytes <= 1 << 34
            and 0 < max_total_bytes <= 1 << 44):
        raise DumpExtractionError("invalid download budget")
    root = Path(staging_dir)
    if root.is_symlink() or not root.is_dir():
        raise DumpExtractionError("unsafe staging directory")
    members = _inventory(Path(inventory), project, date)
    opener = opener if opener is not None else build_opener(NoRedirect())
    total, downloaded, reused, resumed = 0, 0, 0, 0
    for _, _, relative, expected in members:
        target = root
        for component in relative.split("/")[:-1]:
            target = target / component
            if not target.exists():
                target.mkdir()
            if target.is_symlink() or not target.is_dir():
                raise DumpExtractionError("unsafe shard parent directory")
        target = target / relative.rsplit("/", 1)[-1]
        if target.is_symlink():
            raise DumpExtractionError("symlinked staged shard")
        if target.exists():
            source, metadata = _verified_source(target, expected, max_member_bytes)
            source.close()
            size = metadata.st_size
            reused += 1
        else:
            size, action = _download_verified(
                prefix + relative, target, expected, opener,
                max_member_bytes, max_total_bytes - total,
            )
            if action == "resumed":
                resumed += 1
            else:
                downloaded += 1
        total += size
        if total > max_total_bytes:
            raise DumpExtractionError("export exceeds aggregate byte budget")
    return {"members": len(members), "downloaded": downloaded,
            "resumed": resumed, "reused_verified": reused,
            "compressed_bytes": total}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("inventory", type=Path)
    parser.add_argument("project")
    parser.add_argument("month")
    parser.add_argument("staging_dir", type=Path)
    args = parser.parse_args()
    print(stage_verified_members(args.inventory, args.project,
                                 args.month, args.staging_dir))


if __name__ == "__main__":
    main()
