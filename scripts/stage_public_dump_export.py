#!/usr/bin/env python3
"""Stage a complete official current-content export without publishing partial data.

This acquires raw XML shards only. A successful staging directory is NOT an
offline-ready article, rendered page, signed pack, or publisher signature.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import tempfile

from discover_public_dump_inventory import (
    PublicationError, _check_inventory, _official_url,
    discover_completed_export, fetch_official, open_official_stream,
)

MAX_MEMBER_BYTES = 1 << 34
MAX_EXPORT_BYTES = 1 << 44


def _copy_verified(stream, target, expected, budget):
    digest = hashlib.sha256()
    total = 0
    with target.open("xb") as writer:
        with stream as reader:
            while True:
                chunk = reader.read(1024 * 1024)
                if not chunk:
                    break
                total += len(chunk)
                if total > budget:
                    raise PublicationError("member exceeds compressed-byte budget")
                digest.update(chunk)
                writer.write(chunk)
        writer.flush()
        os.fsync(writer.fileno())
    if not total or digest.hexdigest() != expected:
        raise PublicationError("member SHA-256 mismatch")
    return total


def stage_completed_export(project, target, *, fetch=fetch_official,
                           stream=open_official_stream,
                           max_member_bytes=MAX_MEMBER_BYTES,
                           max_total_bytes=MAX_EXPORT_BYTES):
    """Fetch and verify all members before publishing the staging directory."""
    target = Path(target)
    if (not 0 < max_member_bytes <= MAX_EXPORT_BYTES
            or not 0 < max_total_bytes <= MAX_EXPORT_BYTES):
        raise PublicationError("invalid export download budget")
    if target.exists() or target.is_symlink():
        raise PublicationError("staging target already exists")
    parent = target.parent
    if not stat.S_ISDIR(parent.lstat().st_mode):
        raise PublicationError("unsafe staging parent")
    publication, inventory = discover_completed_export(project, fetch)
    _check_inventory(inventory, project, publication["date"])
    members = []
    for line in inventory.decode("utf-8").splitlines():
        digest, relative = line[:64].lower(), line[66:]
        members.append((relative, digest))
    # Only the validated checksum inventory determines what gets downloaded.
    with tempfile.TemporaryDirectory(prefix=".current-export.", dir=parent) as temporary:
        work = Path(temporary)
        total = 0
        for relative, expected in members:
            destination = work / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            url = publication["checksum_url"].removesuffix("SHA256SUMS") + relative
            _official_url(url)
            remaining = max_total_bytes - total
            if remaining <= 0:
                raise PublicationError("export exceeds aggregate compressed-byte budget")
            total += _copy_verified(stream(url), destination, expected,
                                    min(max_member_bytes, remaining))
        inventory_path = work / "SHA256SUMS"
        inventory_path.write_bytes(inventory)
        receipt = {
            "publication": publication,
            "members": len(members),
            "compressed_bytes": total,
            "source": "Wikimedia mediawiki_content_current; unrendered",
            "status": "staged-only; NOT offline-ready",
        }
        (work / "PUBLICATION.json").write_text(
            json.dumps(receipt, sort_keys=True, indent=2) + "\n", encoding="utf-8"
        )
        for item in (inventory_path, work / "PUBLICATION.json"):
            with item.open("rb") as reader:
                os.fsync(reader.fileno())
        if target.exists() or target.is_symlink():
            raise PublicationError("staging target already exists")
        os.rename(work, target)
        parent_fd = os.open(parent, os.O_RDONLY)
        try:
            os.fsync(parent_fd)
        finally:
            os.close(parent_fd)
    return receipt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("project")
    parser.add_argument("private_staging_directory", type=Path)
    args = parser.parse_args()
    receipt = stage_completed_export(args.project, args.private_staging_directory)
    print(json.dumps(receipt, sort_keys=True))


if __name__ == "__main__":
    main()
