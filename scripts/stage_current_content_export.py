#!/usr/bin/env python3
"""Resume and stage a complete SHA-256 Wikimedia current-content XML export.

Caller inventory is revalidated against fresh official SHA256SUMS before I/O.
This never activates content, authenticates a signing key, or renders articles.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import tempfile
from urllib.request import HTTPRedirectHandler, Request, build_opener

from discover_current_content_export import (
    ExportDiscoveryError, fetch_official, inventory_url, parse_checksums,
)

CHUNK = 1024 * 1024
MAX_MEMBER_BYTES = 1024 ** 4
RANGE = re.compile(r"bytes ([0-9]+)-([0-9]+)/([0-9]+)\Z")


class ExportStagingError(ValueError):
    pass


class _NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, msg, headers, newurl):
        raise ExportStagingError("source member redirect rejected")


def _open(request, timeout):
    return build_opener(_NoRedirect()).open(request, timeout=timeout)


def _ordinary_file(path):
    try:
        info = path.lstat()
    except FileNotFoundError:
        return False
    if not stat.S_ISREG(info.st_mode):
        raise ExportStagingError("unsafe staged file or partial")
    return True


def _hash(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(CHUNK), b""):
            digest.update(block)
    return digest.hexdigest()


def _publish(part, target, directory):
    try:
        os.link(part, target, follow_symlinks=False)
    except FileExistsError as error:
        raise ExportStagingError("final member appeared during download") from error
    part.unlink()
    directory_fd = os.open(directory, os.O_RDONLY)
    try:
        os.fsync(directory_fd)
    finally:
        os.close(directory_fd)


def _receipt(item, size):
    return {
        "name": item["path"].rsplit("/", 1)[-1],
        "bytes": size,
        "sha256": item["sha256"],
        "relative_path": item["path"],
        "url": item["url"],
    }


def _transfer(item, directory, *, opener):
    url, digest = item["url"], item["sha256"]
    name = item["path"].rsplit("/", 1)[-1]
    target, part = directory / name, directory / (name + ".part")
    if _ordinary_file(target):
        size = target.stat().st_size
        if not (0 < size <= MAX_MEMBER_BYTES) or _hash(target) != digest:
            raise ExportStagingError("existing member does not match SHA-256 inventory")
        return _receipt(item, size)

    partial_exists = _ordinary_file(part)
    offset = part.stat().st_size if partial_exists else 0
    if partial_exists and offset == 0:
        part.unlink()
    if offset and _hash(part) == digest:
        _publish(part, target, directory)
        return _receipt(item, offset)
    if offset > MAX_MEMBER_BYTES:
        raise ExportStagingError("oversized interrupted transfer")

    request = Request(url, headers={
        "User-Agent": "FoundationWikipedia/0.1 current-content-stager",
        "Accept-Encoding": "identity",
        **({"Range": f"bytes={offset}-"} if offset else {}),
    })
    with opener(request, timeout=40) as response:
        if response.geturl() != url:
            raise ExportStagingError("source URL changed during transfer")
        if response.status != (206 if offset else 200):
            raise ExportStagingError("server ignored or misreported resume range")
        if response.headers.get("Content-Encoding", "identity").lower() != "identity":
            raise ExportStagingError("non-byte-addressable source encoding")
        length = response.headers.get("Content-Length")
        if length is not None and (not length.isdecimal() or int(length) <= 0):
            raise ExportStagingError("invalid member Content-Length")
        if offset:
            match = RANGE.fullmatch(response.headers.get("Content-Range", ""))
            if not match:
                raise ExportStagingError("missing resume range metadata")
            start, end, total = map(int, match.groups())
            if start != offset or end < start or end + 1 != total or total > MAX_MEMBER_BYTES:
                raise ExportStagingError("invalid resumed Content-Range")
            if length is not None and int(length) != total - offset:
                raise ExportStagingError("resumed Content-Length mismatch")
        elif length is not None and int(length) > MAX_MEMBER_BYTES:
            raise ExportStagingError("oversized source member")
        flags = os.O_WRONLY | os.O_APPEND | os.O_CREAT
        if not offset:
            flags |= os.O_EXCL
        flags |= getattr(os, "O_NOFOLLOW", 0)
        fd = os.open(part, flags, 0o600)
        with os.fdopen(fd, "ab") as writer:
            info = os.fstat(writer.fileno())
            if not stat.S_ISREG(info.st_mode) or info.st_size != offset or info.st_nlink != 1:
                raise ExportStagingError("partial member changed during resume")
            count = offset
            for chunk in iter(lambda: response.read(CHUNK), b""):
                count += len(chunk)
                if count > MAX_MEMBER_BYTES:
                    raise ExportStagingError("source exceeded staging byte budget")
                if offset and count > total:
                    raise ExportStagingError("server returned excess range bytes")
                if not offset and length is not None and count > int(length):
                    raise ExportStagingError("source returned excess content bytes")
                writer.write(chunk)
            writer.flush()
            os.fsync(writer.fileno())
    if offset and count != total:
        raise ExportStagingError("interrupted or truncated resumed member")
    if not offset and length is not None and count != int(length):
        raise ExportStagingError("interrupted or truncated source member")
    if count == 0 or _hash(part) != digest:
        part.unlink(missing_ok=True)
        raise ExportStagingError("source member SHA-256 mismatch")
    _publish(part, target, directory)
    return _receipt(item, count)


def _preflight(report, fetcher):
    if not isinstance(report, dict) or report.get("completed") is not True:
        raise ExportStagingError("incomplete or invalid current-content export")
    project, generation = report.get("project"), report.get("generation_id")
    try:
        expected = inventory_url(project, generation)
    except ExportDiscoveryError as error:
        raise ExportStagingError("invalid source project or generation") from error
    if report.get("dataset") != "mediawiki_content_current" or report.get("checksum_inventory_url") != expected:
        raise ExportStagingError("mismatched SHA-256 inventory identity")
    try:
        fresh = parse_checksums(fetcher(expected), project, generation)
    except (ExportDiscoveryError, OSError, UnicodeError) as error:
        raise ExportStagingError("official SHA-256 inventory unavailable or invalid") from error
    if report.get("files") != fresh["files"]:
        raise ExportStagingError("discovery report does not match current published SHA-256 inventory")
    names = set()
    for item in fresh["files"]:
        name = item["path"].rsplit("/", 1)[-1].lower()
        if name in names:
            raise ExportStagingError("case-colliding output filenames")
        names.add(name)
    return fresh


def stage_export(report, directory, output_manifest, *, fetcher=fetch_official, opener=_open):
    """Stage complete verified inventory before no-clobber manifest publication."""
    fresh = _preflight(report, fetcher)
    directory = Path(directory)
    output = Path(output_manifest)
    if directory.is_symlink() or output.parent.is_symlink() or not output.parent.is_dir():
        raise ExportStagingError("unsafe staging or output directory")
    directory.mkdir(parents=True, exist_ok=True)
    if output.exists() or output.is_symlink():
        raise ExportStagingError("output manifest already exists")
    entries = [_transfer(item, directory, opener=opener) for item in fresh["files"]]
    entries.sort(key=lambda item: item["name"])
    manifest = {
        "project": fresh["project"], "generation_id": fresh["generation_id"],
        "source_url": fresh["checksum_inventory_url"], "completed": True,
        "files": entries,
    }
    for item in entries:
        path = directory / item["name"]
        if not _ordinary_file(path) or path.stat().st_size != item["bytes"] or _hash(path) != item["sha256"]:
            raise ExportStagingError("staged member changed before manifest publication")
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", dir=output.parent,
            prefix=".current-content-manifest.", suffix=".tmp", delete=False,
        ) as handle:
            temporary = Path(handle.name)
            json.dump(manifest, handle, indent=2, sort_keys=True)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
        try:
            os.link(temporary, output)
        except FileExistsError as error:
            raise ExportStagingError("manifest output appeared during publication") from error
        fd = os.open(output.parent, os.O_RDONLY)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("completed_export_report", type=Path)
    parser.add_argument("staging_directory", type=Path)
    parser.add_argument("source_manifest", type=Path)
    args = parser.parse_args()
    try:
        report = json.loads(args.completed_export_report.read_text(encoding="utf-8"))
        manifest = stage_export(report, args.staging_directory, args.source_manifest)
    except (ExportStagingError, OSError, ValueError) as error:
        parser.exit(2, f"Current-content staging failed: {error}\n")
    print(f"Staged {len(manifest['files'])} SHA-256-verified member(s), without activation")


if __name__ == "__main__":
    main()
