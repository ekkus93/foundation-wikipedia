#!/usr/bin/env python3
"""Stage every member of a completed Wikimedia public dump and finalize hashes.

Network transfers resume safely; --local-directory instead uses offline files.
No source is activated and no publisher signature is claimed.
"""
import argparse
import json
from pathlib import Path

from download_public_dump import (
    HOST, DownloadError, _metadata, fetch_dump_member,
)
from finalize_public_dump_staging import finalize_public_dump, save_manifest
from stage_local_dump import stage_local_dump_member


def stage_generation(report, staging, output_manifest, *, local_directory=None,
                     user_agent=None, opener=None):
    """Verify the full member inventory before I/O; finalize only if all succeed."""
    if not isinstance(report, dict) or report.get("completed") is not True:
        raise DownloadError("incomplete public dump discovery report")
    project, generation = report.get("project"), report.get("generation_id")
    if report.get("status_url") != f"{HOST}/{project}/{generation}/dumpstatus.json":
        raise DownloadError("unexpected official dump status URL")
    files = report.get("files")
    if not isinstance(files, list) or not (0 < len(files) <= 100000):
        raise DownloadError("missing or excessive source members")
    seen = set()
    names = []
    for entry in files:
        if not isinstance(entry, dict):
            raise DownloadError("invalid source member metadata")
        name = entry.get("name")
        _metadata(report, name)
        if name.lower() in seen:
            raise DownloadError("duplicate or case-colliding source members")
        seen.add(name.lower())
        names.append(name)
    staging = Path(staging)
    if local_directory is not None:
        local_directory = Path(local_directory)
        if local_directory.is_symlink() or not local_directory.is_dir():
            raise DownloadError("missing or unsafe local source directory")
    elif not isinstance(user_agent, str) or len(user_agent.strip()) < 10:
        raise DownloadError("network transfer requires descriptive --user-agent")

    for name in sorted(names):
        if local_directory is None:
            fetch_dump_member(report, name, staging, user_agent, opener=opener)
        else:
            stage_local_dump_member(report, name, local_directory / name, staging)
    manifest = finalize_public_dump(report, staging)
    save_manifest(manifest, output_manifest)
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("completed_report", type=Path)
    parser.add_argument("staging_directory", type=Path)
    parser.add_argument("output_manifest", type=Path)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--local-directory", type=Path)
    group.add_argument("--user-agent")
    args = parser.parse_args()
    report = json.loads(args.completed_report.read_text(encoding="utf-8"))
    manifest = stage_generation(
        report, args.staging_directory, args.output_manifest,
        local_directory=args.local_directory, user_agent=args.user_agent,
    )
    print(f"Staged {len(manifest['files'])} source member(s); "
          "SHA-256 manifest written. Publication authenticity not proven.")


if __name__ == "__main__":
    main()
