#!/usr/bin/env python3
"""Resume official HTTPS source downloads into staging; never activate a snapshot.

Caller-supplied expected checksums are not publisher authentication. Only use
release manifests authenticated independently against official Wikimedia data.
"""
import argparse
import hashlib
import json
import os
import re
import stat
from pathlib import Path
from urllib.parse import quote, urlsplit
from urllib.request import HTTPRedirectHandler, Request, build_opener

from verify_source_staging import (
    CHUNK, HEX_SHA256, IDENTITY, MAX_MEMBERS, NAME,
    SourceVerificationError, _official_url, _valid_name, verify_source_bytes,
)

RANGE = re.compile(r"^bytes (\d+)-(\d+)/(\d+)$")


class SourceDownloadError(SourceVerificationError):
    pass


class _RejectRedirects(HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, msg, headers, newurl):
        raise SourceDownloadError("source redirects require explicit approval")


def _open_official(request, timeout):
    return build_opener(_RejectRedirects()).open(request, timeout=timeout)


def _preflight(manifest):
    """Validate the complete generation before network or filesystem mutation."""
    if not isinstance(manifest, dict):
        raise SourceDownloadError("invalid manifest")
    base = manifest.get("source_url")
    if (manifest.get("completed") is not True
            or not isinstance(manifest.get("project"), str)
            or not IDENTITY.fullmatch(manifest["project"])
            or not isinstance(manifest.get("generation_id"), str)
            or not NAME.fullmatch(manifest["generation_id"])
            or manifest["generation_id"].endswith(".")):
        raise SourceDownloadError("incomplete or unsafe generation")
    if not _official_url(base, manifest["project"], manifest["generation_id"]):
        raise SourceDownloadError("untrusted source URL")
    parsed = urlsplit(base)
    if not parsed.path.endswith("/") or parsed.query:
        raise SourceDownloadError("source URL must be an official directory")
    files = manifest.get("files")
    if not isinstance(files, list) or not (0 < len(files) <= MAX_MEMBERS):
        raise SourceDownloadError("invalid member count")
    seen = set()
    for item in files:
        if not isinstance(item, dict):
            raise SourceDownloadError("invalid member metadata")
        name, digest, size = item.get("name"), item.get("sha256"), item.get("bytes")
        if (not _valid_name(name)
                or not isinstance(digest, str) or not HEX_SHA256.fullmatch(digest)
                or type(size) is not int or size <= 0):
            raise SourceDownloadError("unsafe member metadata")
        if name.lower() in seen:
            raise SourceDownloadError("duplicate source member")
        seen.add(name.lower())
    return base, files


def _matches(path, size, digest):
    if path.is_symlink() or not path.is_file():
        raise SourceDownloadError("unsafe staged member")
    if path.stat().st_size != size:
        return False
    actual = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(CHUNK), b""):
            actual.update(block)
    return actual.hexdigest() == digest


def _response(base, name, offset, size, opener, timeout):
    url = base + quote(name, safe="")
    headers = {"Accept-Encoding": "identity"}
    if offset:
        headers["Range"] = f"bytes={offset}-"
    response = opener(Request(url, headers=headers), timeout=timeout)
    try:
        final, expected = urlsplit(response.geturl()), urlsplit(url)
        if (final.scheme != "https" or final.hostname != expected.hostname
                or final.port not in (None, 443)
                or final.username is not None or final.password is not None):
            raise SourceDownloadError("redirect to untrusted origin")
        if response.headers.get("Content-Encoding", "identity").lower() != "identity":
            raise SourceDownloadError("compressed transfer is not byte-addressable")
        if response.status != (206 if offset else 200):
            raise SourceDownloadError("server did not honor resume range")
        if offset:
            match = RANGE.fullmatch(response.headers.get("Content-Range", ""))
            if not match or tuple(map(int, match.groups())) != (offset, size - 1, size):
                raise SourceDownloadError("incorrect Content-Range")
        length = response.headers.get("Content-Length")
        if length is not None:
            try:
                actual = int(length)
            except ValueError as error:
                raise SourceDownloadError("invalid Content-Length") from error
            if actual != size - offset:
                raise SourceDownloadError("incorrect Content-Length")
        return response
    except Exception:
        response.close()
        raise


def _download(base, item, directory, opener, timeout):
    name, size, digest = item["name"], item["bytes"], item["sha256"]
    target, partial = directory / name, directory / (name + ".part")
    if target.exists() or target.is_symlink():
        if not _matches(target, size, digest):
            raise SourceDownloadError(f"existing staged file differs: {name}")
        return
    if partial.is_symlink():
        raise SourceDownloadError("unsafe partial symlink")
    offset = 0
    if partial.exists():
        if not partial.is_file() or partial.stat().st_nlink != 1:
            raise SourceDownloadError("unsafe partial file")
        offset = partial.stat().st_size
        if offset > size:
            raise SourceDownloadError("oversized partial file")
    if offset < size:
        with _response(base, name, offset, size, opener, timeout) as response:
            flags = os.O_WRONLY | os.O_CREAT | os.O_APPEND
            if hasattr(os, "O_NOFOLLOW"):
                flags |= os.O_NOFOLLOW
            fd = os.open(partial, flags, 0o600)
            with os.fdopen(fd, "ab") as handle:
                st = os.fstat(handle.fileno())
                if (not stat.S_ISREG(st.st_mode) or st.st_nlink != 1
                        or st.st_size != offset):
                    raise SourceDownloadError("partial changed during transfer")
                written = offset
                for block in iter(lambda: response.read(CHUNK), b""):
                    written += len(block)
                    if written > size:
                        raise SourceDownloadError("server sent excess bytes")
                    handle.write(block)
                handle.flush()
                os.fsync(handle.fileno())
    if partial.stat().st_size != size:
        raise SourceDownloadError(f"incomplete transfer; retry to resume: {name}")
    if not _matches(partial, size, digest):
        partial.unlink()
        raise SourceDownloadError(f"checksum mismatch: {name}")
    os.replace(partial, target)


def stage_generation(manifest, directory, opener=None, timeout=30):
    """Stage complete verified bytes; never touch an active snapshot."""
    base, files = _preflight(manifest)
    if opener is None:
        opener = _open_official
    directory = Path(directory)
    if directory.is_symlink() or not directory.is_dir():
        raise SourceDownloadError("staging directory missing or unsafe")
    if timeout <= 0:
        raise SourceDownloadError("timeout must be positive")
    for item in files:
        _download(base, item, directory, opener, timeout)
    return verify_source_bytes(manifest, directory)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("staging_directory", type=Path)
    parser.add_argument("--timeout", type=float, default=30)
    args = parser.parse_args()
    count = stage_generation(
        json.loads(args.manifest.read_text(encoding="utf-8")),
        args.staging_directory, timeout=args.timeout,
    )
    print(f"Verified {count} staged file(s); upstream authenticity not established.")


if __name__ == "__main__":
    main()
