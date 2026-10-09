#!/usr/bin/env python3
"""Resume one official public Wikimedia dump file into a staging directory.

Requires a separately discovered *completed* dump report. HTTP status,
ranges, byte lengths and upstream SHA-1 are checked before promotion;
a local SHA-256 receipt is also returned. This does not establish a signed
publisher trust chain or install any snapshot.
"""
import argparse
from datetime import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import stat
from urllib.request import Request, build_opener, HTTPRedirectHandler

HOST = "https://dumps.wikimedia.org"
PROJECT = re.compile(r"^[a-z0-9_]+$")
DATE = re.compile(r"^[0-9]{8}$")
NAME = re.compile(r"^[A-Za-z0-9_-][A-Za-z0-9_.-]*$")
SHA1 = re.compile(r"^[0-9a-fA-F]{40}$")
RANGE = re.compile(r"^bytes ([0-9]+)-([0-9]+)/([0-9]+)$")
MAX_BYTES = 1024 ** 4
CHUNK = 1024 * 1024


class DownloadError(ValueError):
    pass


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, msg, headers, newurl):
        raise DownloadError("HTTP redirects rejected for authoritative dump transport")


def _safe_name(name):
    if not isinstance(name, str) or not NAME.fullmatch(name) or name.endswith("."):
        return False
    base = name.split(".")[0].upper()
    return base not in {"CON", "PRN", "AUX", "NUL"} and not (
        len(base) == 4 and base[:3] in {"COM", "LPT"} and base[3] in "123456789"
    )


def _metadata(report, name):
    if not isinstance(report, dict) or report.get("completed") is not True:
        raise DownloadError("source generation is incomplete")
    project, generation = report.get("project"), report.get("generation_id")
    if not isinstance(project, str) or not PROJECT.fullmatch(project):
        raise DownloadError("invalid project")
    if not isinstance(generation, str) or not DATE.fullmatch(generation):
        raise DownloadError("invalid generation")
    try:
        datetime.strptime(generation, "%Y%m%d")
    except ValueError as error:
        raise DownloadError("invalid generation date") from error
    if not _safe_name(name):
        raise DownloadError("unsafe member filename")
    members = report.get("files")
    if not isinstance(members, list):
        raise DownloadError("invalid file list")
    files = [entry for entry in members if isinstance(entry, dict) and entry.get("name") == name]
    if len(files) != 1:
        raise DownloadError("missing or duplicate member")
    entry = files[0]
    size, upstream_sha1 = entry.get("bytes"), entry.get("sha1")
    url = f"{HOST}/{project}/{generation}/{name}"
    if (
        type(size) is not int
        or not (0 < size <= MAX_BYTES)
        or not isinstance(upstream_sha1, str)
        or not SHA1.fullmatch(upstream_sha1)
        or entry.get("url") != url
    ):
        raise DownloadError("invalid size, upstream checksum or official URL")
    return url, size, upstream_sha1.lower()


def _hash_file(path):
    digest1, digest256 = hashlib.sha1(), hashlib.sha256()
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0)
    try:
        descriptor = os.open(path, flags)
        with os.fdopen(descriptor, "rb") as stream:
            info = os.fstat(stream.fileno())
            if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
                raise DownloadError("unsafe dump member file type or hardlink")
            for block in iter(lambda: stream.read(CHUNK), b""):
                digest1.update(block)
                digest256.update(block)
    except OSError as error:
        raise DownloadError("dump member changed during verification") from error
    return digest1.hexdigest(), digest256.hexdigest()


def _ordinary_file(path):
    try:
        info = path.lstat()
    except FileNotFoundError:
        return False
    if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
        raise DownloadError("unsafe existing destination or partial file (type or hardlink)")
    return True


def fetch_dump_member(report, name, dest, user_agent, opener=None):
    """Return a local byte/hash receipt; resume .part files without truncating."""
    url, size, expected_sha1 = _metadata(report, name)
    if not isinstance(user_agent, str) or len(user_agent.strip()) < 10:
        raise DownloadError("a descriptive Wikimedia user agent is required")
    dest = Path(dest)
    if dest.is_symlink():
        raise DownloadError("staging destination is a symlink")
    dest.mkdir(parents=True, exist_ok=True)
    target, partial = dest / name, dest / (name + ".part")

    if _ordinary_file(target):
        if target.stat().st_size != size:
            raise DownloadError("existing final member size mismatch")
        observed_sha1, observed_sha256 = _hash_file(target)
        if observed_sha1 != expected_sha1:
            raise DownloadError("existing final member SHA-1 mismatch")
        return {"path": str(target), "bytes": size, "sha1": observed_sha1, "sha256": observed_sha256}

    partial_exists = _ordinary_file(partial)
    offset = partial.stat().st_size if partial_exists else 0
    if offset == 0 and partial_exists:
        # A transfer may fail before receiving its first byte.
        partial.unlink()
    if offset > size:
        raise DownloadError("oversized partial member")
    if offset < size:
        headers = {"User-Agent": user_agent, "Accept-Encoding": "identity"}
        if offset:
            headers["Range"] = f"bytes={offset}-"
        request = Request(url, headers=headers)
        client = opener if opener is not None else build_opener(NoRedirect())
        with client.open(request, timeout=30) as response:
            status = response.status
            if hasattr(response, "geturl") and response.geturl() != url:
                raise DownloadError("transport changed the authorized URL")
            if status != (206 if offset else 200):
                raise DownloadError("unexpected HTTP status for resume")
            if response.headers.get("Content-Encoding", "identity").lower() != "identity":
                raise DownloadError("non-byte-addressable dump Content-Encoding")
            length = response.headers.get("Content-Length")
            expected_remaining = size - offset
            if length is not None and length != str(expected_remaining):
                raise DownloadError("unexpected content length")
            if offset:
                match = RANGE.fullmatch(response.headers.get("Content-Range", ""))
                if match is None or tuple(map(int, match.groups())) != (
                    offset, size - 1, size
                ):
                    raise DownloadError("invalid resumed Content-Range")
            flags = os.O_WRONLY | os.O_APPEND | os.O_CREAT
            if not offset:
                flags |= os.O_EXCL
            flags |= getattr(os, "O_NOFOLLOW", 0)
            descriptor = os.open(partial, flags, 0o600)
            with os.fdopen(descriptor, "ab") as writer:
                info = os.fstat(writer.fileno())
                if (not stat.S_ISREG(info.st_mode) or info.st_nlink != 1
                        or info.st_size != offset):
                    raise DownloadError("partial dump member changed during resume")
                count = offset
                while True:
                    data = response.read(CHUNK)
                    if not data:
                        break
                    count += len(data)
                    if count > size:
                        raise DownloadError("upstream delivered too many bytes")
                    writer.write(data)
                writer.flush()
                os.fsync(writer.fileno())
            if count != size:
                raise DownloadError("incomplete upstream response")

    if partial.stat().st_size != size:
        raise DownloadError("incomplete staged member")
    observed_sha1, observed_sha256 = _hash_file(partial)
    if observed_sha1 != expected_sha1:
        partial.unlink()
        raise DownloadError("upstream SHA-1 mismatch")
    # An atomic no-clobber promotion: never overwrite an already-staged member.
    try:
        os.link(partial, target, follow_symlinks=False)
    except FileExistsError as error:
        raise DownloadError("final member appeared during download") from error
    partial.unlink()
    directory_fd = os.open(dest, os.O_RDONLY)
    try:
        os.fsync(directory_fd)
    finally:
        os.close(directory_fd)
    return {"path": str(target), "bytes": size, "sha1": observed_sha1, "sha256": observed_sha256}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("completed_report", type=Path)
    parser.add_argument("filename")
    parser.add_argument("staging_directory", type=Path)
    parser.add_argument("--user-agent", required=True)
    args = parser.parse_args()
    report = json.loads(args.completed_report.read_text(encoding="utf-8"))
    receipt = fetch_dump_member(
        report, args.filename, args.staging_directory, args.user_agent
    )
    print(json.dumps(receipt, sort_keys=True))


if __name__ == "__main__":
    main()
