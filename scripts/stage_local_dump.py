#!/usr/bin/env python3
"""Stage a locally obtained official Wikimedia dump member without network access.

Requires a completed official discovery report with the upstream SHA-1 for
this member. This verifies transport integrity, not publisher signatures.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import tempfile

from download_public_dump import CHUNK, DownloadError, _hash_file, _metadata, _ordinary_file


def stage_local_dump_member(report, name, source, destination):
    """Copy a verified local dump member into staging, without replacing files."""
    _url, expected_size, expected_sha1 = _metadata(report, name)
    source = Path(source)
    if not _ordinary_file(source):
        raise DownloadError("local dump member does not exist")
    if source.stat().st_size != expected_size:
        raise DownloadError("local dump member size mismatch")

    destination = Path(destination)
    if destination.is_symlink():
        raise DownloadError("staging destination is a symlink")
    destination.mkdir(parents=True, exist_ok=True)
    target = destination / name
    if _ordinary_file(target):
        if target.stat().st_size != expected_size:
            raise DownloadError("existing final member size mismatch")
        sha1, sha256 = _hash_file(target)
        if sha1 != expected_sha1:
            raise DownloadError("existing final member SHA-1 mismatch")
        return {"path": str(target), "bytes": expected_size, "sha1": sha1, "sha256": sha256}

    # A unique private temporary file never touches an interrupted HTTP .part
    # transfer. It is not made visible as a final member until hash validation.
    temporary_path = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="wb", prefix="." + name + ".", suffix=".local-part",
            dir=destination, delete=False
        ) as writer:
            temporary_path = Path(writer.name)
            sha1 = hashlib.sha1()
            sha256 = hashlib.sha256()
            count = 0
            with source.open("rb") as reader:
                while True:
                    block = reader.read(CHUNK)
                    if not block:
                        break
                    count += len(block)
                    if count > expected_size:
                        raise DownloadError("local dump member grew during copy")
                    writer.write(block)
                    sha1.update(block)
                    sha256.update(block)
            writer.flush()
            os.fsync(writer.fileno())
        if count != expected_size:
            raise DownloadError("local dump member changed size during copy")
        if sha1.hexdigest() != expected_sha1:
            raise DownloadError("local dump member upstream SHA-1 mismatch")
        # Atomic no-clobber publication; a competing verified transfer wins.
        try:
            os.link(temporary_path, target)
        except FileExistsError as error:
            raise DownloadError("final member appeared during local staging") from error
        directory_fd = os.open(destination, os.O_RDONLY)
        try:
            os.fsync(directory_fd)
        finally:
            os.close(directory_fd)
        return {
            "path": str(target), "bytes": count,
            "sha1": sha1.hexdigest(), "sha256": sha256.hexdigest(),
        }
    finally:
        if temporary_path is not None:
            temporary_path.unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("completed_report", type=Path)
    parser.add_argument("filename")
    parser.add_argument("local_file", type=Path)
    parser.add_argument("staging_directory", type=Path)
    args = parser.parse_args()
    report = json.loads(args.completed_report.read_text(encoding="utf-8"))
    receipt = stage_local_dump_member(
        report, args.filename, args.local_file, args.staging_directory
    )
    print(json.dumps(receipt, sort_keys=True))


if __name__ == "__main__":
    main()
