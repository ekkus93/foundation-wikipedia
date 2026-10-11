#!/usr/bin/env python3
"""Stage checksum-verified official current-content XML shards.

Completed shards survive restart; incomplete transfers are discarded.
Caller must authenticate the inventory before using this transport.
"""
import argparse
import hashlib
import os
from pathlib import Path
import tempfile
from urllib.error import HTTPError, URLError
from urllib.request import Request, build_opener

from discover_public_current_export import NoRedirect, official_url
from extract_public_dump_export import DumpExtractionError, _inventory
from extract_public_dump_pages import _verified_source


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
    total, downloaded, reused = 0, 0, 0
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
            total += metadata.st_size
            reused += 1
        else:
            url = prefix + relative
            temporary = None
            try:
                try:
                    with opener.open(Request(url, headers={
                        "User-Agent": "FoundationWikipedia/0.1",
                    }), timeout=30) as response:
                        if response.status != 200 or response.geturl() != url:
                            raise DumpExtractionError("unexpected official shard response")
                        digest = hashlib.sha256()
                        size = 0
                        with tempfile.NamedTemporaryFile(
                            mode="wb", dir=target.parent,
                            prefix=".wiki-shard.", delete=False,
                        ) as writer:
                            temporary = Path(writer.name)
                            while chunk := response.read(1024 * 1024):
                                size += len(chunk)
                                if size > max_member_bytes or total + size > max_total_bytes:
                                    raise DumpExtractionError("shard exceeds download budget")
                                writer.write(chunk)
                                digest.update(chunk)
                            writer.flush()
                            os.fsync(writer.fileno())
                except (HTTPError, URLError, TimeoutError) as error:
                    raise DumpExtractionError("official shard HTTPS failure") from error
                if size == 0 or digest.hexdigest() != expected:
                    raise DumpExtractionError("shard SHA-256 mismatch")
                try:
                    os.link(temporary, target)
                except FileExistsError:
                    raise DumpExtractionError("staged shard appeared concurrently") from None
                directory = os.open(target.parent, os.O_RDONLY)
                try:
                    os.fsync(directory)
                finally:
                    os.close(directory)
                total += size
                downloaded += 1
            finally:
                if temporary is not None:
                    temporary.unlink(missing_ok=True)
        if total > max_total_bytes:
            raise DumpExtractionError("export exceeds aggregate byte budget")
    return {"members": len(members), "downloaded": downloaded,
            "reused_verified": reused, "compressed_bytes": total}


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
