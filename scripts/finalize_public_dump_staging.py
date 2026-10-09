#!/usr/bin/env python3
"""Produce a complete SHA-256 staging manifest from a finished public dump.

Each declared member must already be staged and match official reported SHA-1.
This tool does not authenticate the caller-supplied report or activate content.
"""
import argparse
import json
import os
from pathlib import Path
import tempfile

from download_public_dump import (
    HOST, DownloadError, _hash_file, _metadata, _ordinary_file,
)


def finalize_public_dump(report, staging):
    """Verify every reported member and return a complete SHA-256 manifest."""
    if not isinstance(report, dict):
        raise DownloadError("invalid completed discovery report")
    project = report.get("project")
    generation = report.get("generation_id")
    if not isinstance(project, str) or not isinstance(generation, str):
        raise DownloadError("missing official dump generation")
    status_url = f"{HOST}/{project}/{generation}/dumpstatus.json"
    if report.get("status_url") != status_url:
        raise DownloadError("unexpected official dump status URL")
    files = report.get("files")
    if not isinstance(files, list) or not (0 < len(files) <= 100000):
        raise DownloadError("missing or excessive completed dump members")
    staging = Path(staging)
    if staging.is_symlink() or not staging.is_dir():
        raise DownloadError("missing or unsafe staging directory")
    names = set()
    manifest_files = []
    for entry in files:
        if not isinstance(entry, dict):
            raise DownloadError("invalid completed dump member")
        name = entry.get("name")
        _url, expected_size, expected_sha1 = _metadata(report, name)
        if name.lower() in names:
            raise DownloadError("duplicate or case-colliding dump members")
        names.add(name.lower())
        path = staging / name
        if not _ordinary_file(path):
            raise DownloadError(f"missing staged dump member: {name}")
        if path.stat().st_size != expected_size:
            raise DownloadError(f"staged dump member size mismatch: {name}")
        sha1, sha256 = _hash_file(path)
        if sha1 != expected_sha1:
            raise DownloadError(f"staged dump member upstream SHA-1 mismatch: {name}")
        manifest_files.append({"name": name, "bytes": expected_size, "sha256": sha256})
    manifest_files.sort(key=lambda entry: entry["name"])
    return {
        "project": project,
        "generation_id": generation,
        "source_url": status_url,
        "completed": True,
        "files": manifest_files,
    }


def save_manifest(manifest, output):
    """Publish a manifest without overwriting any previous verification record."""
    output = Path(output)
    if output.parent.is_symlink() or not output.parent.is_dir():
        raise DownloadError("manifest output parent is unsafe")
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", dir=output.parent,
            prefix=".source-manifest.", suffix=".tmp", delete=False,
        ) as writer:
            temporary = Path(writer.name)
            json.dump(manifest, writer, sort_keys=True, indent=2)
            writer.write("\n")
            writer.flush()
            os.fsync(writer.fileno())
        try:
            os.link(temporary, output)
        except FileExistsError as error:
            raise DownloadError("manifest output already exists") from error
        directory_fd = os.open(output.parent, os.O_RDONLY)
        try:
            os.fsync(directory_fd)
        finally:
            os.close(directory_fd)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("completed_report", type=Path)
    parser.add_argument("staging_directory", type=Path)
    parser.add_argument("output_manifest", type=Path)
    args = parser.parse_args()
    report = json.loads(args.completed_report.read_text(encoding="utf-8"))
    manifest = finalize_public_dump(report, args.staging_directory)
    save_manifest(manifest, args.output_manifest)
    print(f"Verified {len(manifest['files'])} staged member(s). "
          "Upstream SHA-1 is not signed publisher authentication.")


if __name__ == "__main__":
    main()
