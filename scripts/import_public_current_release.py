#!/usr/bin/env python3
"""Import one completed official current-content release into unrendered JSONL.

This is a resumable source-acquisition pipeline, NOT a Parsoid renderer,
canonical article installer or source-publisher signature verifier.
"""
import argparse
import json
import os
from pathlib import Path
import stat

from discover_public_current_export import discover_completed_export, official_url
from extract_public_dump_export import DumpExtractionError, _inventory, extract_current_export
from stage_public_current_shards import stage_official_verified_members


def _private_directory(path):
    try:
        info = path.lstat()
    except FileNotFoundError:
        raise DumpExtractionError("working directory must exist") from None
    if not stat.S_ISDIR(info.st_mode):
        raise DumpExtractionError("unsafe working directory")
    return path


def import_official_current_release(project, month, work_dir, output_jsonl,
                                    *, opener=None, max_member_bytes=1 << 30,
                                    max_total_bytes=1 << 44,
                                    max_total_pages=10_000_000):
    """Orchestrate live inventory revalidation, restart-safe staged shards and XML.

    Caller owns a private work directory with no untrusted concurrent writers.
    Existing final output is never overwritten and no partial export is published.
    """
    url = official_url(project, month)
    root, output = Path(work_dir), Path(output_jsonl)
    _private_directory(root)
    if (output.exists() or output.is_symlink()
            or not output.parent.is_dir() or output.parent.is_symlink()):
        raise DumpExtractionError("unsafe or existing output")
    inventory = root / "SHA256SUMS"
    shards = root / "shards"
    if inventory.is_symlink():
        raise DumpExtractionError("unsafe checksum inventory")
    if inventory.exists():
        _inventory(inventory, project, month)
    else:
        discover_completed_export(project, month, inventory, opener=opener)
    if shards.exists() or shards.is_symlink():
        _private_directory(shards)
    else:
        shards.mkdir(mode=0o700)
    # This preflight re-downloads SHA256SUMS from the exact official HTTPS URL.
    # Caller-controlled, self-consistent inventories may not bypass it.
    staged = stage_official_verified_members(
        inventory, project, month, shards, opener=opener,
        max_member_bytes=max_member_bytes, max_total_bytes=max_total_bytes,
    )
    extracted = extract_current_export(
        shards, inventory, project, month, output,
        max_member_bytes=max_member_bytes,
        max_total_bytes=max_total_bytes,
        max_total_pages=max_total_pages,
    )
    # This is only a transport/integrity result. No HTML, media, or signature.
    return {
        "source_url": url,
        "project": project,
        "month": month,
        "checksum_algorithm": "sha256",
        "source_type": "mediawiki_content_current",
        "unrendered_pages": extracted,
        "offline_ready": False,
        "publisher_signature_verified": False,
        "staging": staged,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("project")
    parser.add_argument("month")
    parser.add_argument("private_work_directory", type=Path)
    parser.add_argument("output_jsonl", type=Path)
    args = parser.parse_args()
    print(json.dumps(import_official_current_release(
        args.project, args.month, args.private_work_directory, args.output_jsonl,
    ), sort_keys=True))


if __name__ == "__main__":
    main()
