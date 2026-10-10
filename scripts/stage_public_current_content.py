#!/usr/bin/env python3
"""All-or-nothing staging of verified public current-content XML pages as NDJSON."""
from dataclasses import asdict
import json
import os
from pathlib import Path
import tempfile

from import_public_current_content import iter_verified_pages, PublicDumpError


def stage_verified_pages(source_path, expected_sha256, target_path):
    """Publish a completed NDJSON file only after the entire dump validates.

    The caller chooses a private staging directory; this never activates a
    canonical snapshot. Existing target files are never replaced.
    """
    target = Path(target_path)
    if target.exists() or target.is_symlink():
        raise PublicDumpError("staged target already exists")
    parent = target.parent
    if not parent.is_dir() or parent.is_symlink():
        raise PublicDumpError("invalid staging directory")
    temp_path = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", dir=parent,
            prefix=".current-content-", suffix=".part", delete=False,
        ) as output:
            temp_path = Path(output.name)
            count = 0
            for page in iter_verified_pages(source_path, expected_sha256):
                output.write(json.dumps(asdict(page), sort_keys=True, ensure_ascii=False))
                output.write("\n")
                count += 1
            output.flush()
            os.fsync(output.fileno())
        if count == 0:
            raise PublicDumpError("empty public current-content export")
        try:
            os.link(temp_path, target, follow_symlinks=False)
        except FileExistsError as error:
            raise PublicDumpError("staged target appeared during import") from error
        temp_path.unlink()
        temp_path = None
        directory_fd = os.open(parent, os.O_RDONLY)
        try:
            os.fsync(directory_fd)
        finally:
            os.close(directory_fd)
        return count
    finally:
        if temp_path is not None:
            temp_path.unlink(missing_ok=True)
