#!/usr/bin/env python3
"""Fail-closed verification of a *local staging copy* against declared checksums.

Upstream authority and completeness must be established independently: a JSON
manifest written by an attacker is not proof of Wikimedia publication.
"""
import argparse
import hashlib
import json
import os
import re
import stat
from pathlib import Path
from urllib.parse import urlsplit

HEX_SHA256 = re.compile(r"^[0-9a-f]{64}$")
IDENTITY = re.compile(r"^[a-z0-9_]+$")
NAME = re.compile(r"^[a-zA-Z0-9_-][a-zA-Z0-9_.-]*$")
GENERATION = re.compile(r"^[a-zA-Z0-9_-][a-zA-Z0-9_.-]*$")
MAX_MEMBERS = 100000
# Explicit budgets for untrusted manifest-declared compressed input. The
# aggregate bound applies across all members, not separately per shard.
MAX_MEMBER_BYTES = 64 * 1024**3
MAX_TOTAL_BYTES = 4 * 1024**4
MAX_MANIFEST_BYTES = 32 * 1024**2
CHUNK = 1024 * 1024


class SourceVerificationError(ValueError):
    pass


def _official_url(value, project, generation):
    if not isinstance(value, str):
        return False
    try:
        url = urlsplit(value)
        return (
            url.scheme == "https"
            and url.hostname in {"dumps.wikimedia.org", "enterprise.wikimedia.com"}
            and url.port in (None, 443)
            and url.username is None
            and url.password is None
            and not url.query
            and not url.fragment
            and not any(char.isspace() or ord(char) < 32 or ord(char) == 92 for char in value)
            and (
                (url.hostname == "dumps.wikimedia.org"
                 and url.path in {
                     f"/{project}/{generation}/",
                     f"/{project}/{generation}/dumpstatus.json",
                 })
                or (url.hostname == "dumps.wikimedia.org"
                    and url.path == (
                        f"/other/mediawiki_content_current/{project}/{generation}"
                        "/xml/bzip2/SHA256SUMS"
                    ))
                or (url.hostname == "enterprise.wikimedia.com"
                    and bool(url.path.strip("/")))
            )
        )
    except ValueError:
        return False


def _valid_name(name):
    if not isinstance(name, str) or not NAME.fullmatch(name):
        return False
    if name in {".", ".."} or name.endswith("."):
        return False
    base = name.split(".")[0].upper()
    if base in {"CON", "PRN", "AUX", "NUL"}:
        return False
    if len(base) == 4 and base[:3] in {"COM", "LPT"} and base[3] in "123456789":
        return False
    return True


def verify_source_bytes(manifest, directory):
    """Return verified member count, raising for any unsafe or mismatched bytes."""
    directory = Path(directory)
    if (directory.is_symlink() or not directory.is_dir()
            or any(parent.is_symlink() for parent in directory.parents)):
        raise SourceVerificationError("staging directory is missing or unsafe")
    if os.name == "posix" and directory.stat().st_mode & 0o022:
        raise SourceVerificationError("staging directory is writable by other users")
    if not isinstance(manifest, dict):
        raise SourceVerificationError("manifest must be a JSON object")
    if (
        not isinstance(manifest.get("completed"), bool)
        or manifest["completed"] is not True
        or not isinstance(manifest.get("project"), str)
        or not IDENTITY.fullmatch(manifest["project"])
        or not isinstance(manifest.get("generation_id"), str)
        or not GENERATION.fullmatch(manifest["generation_id"])
        or manifest["generation_id"].endswith(".")
        or not _official_url(manifest.get("source_url"), manifest["project"], manifest["generation_id"])
    ):
        raise SourceVerificationError("invalid or incomplete upstream generation metadata")
    members = manifest.get("files")
    if not isinstance(members, list) or not (0 < len(members) <= MAX_MEMBERS):
        raise SourceVerificationError("manifest must declare a bounded nonempty file set")
    seen = set()
    declared = set()
    validated = []
    total_bytes = 0
    for item in members:
        if not isinstance(item, dict):
            raise SourceVerificationError("invalid file metadata")
        name = item.get("name")
        digest = item.get("sha256")
        size = item.get("bytes")
        if (
            not _valid_name(name)
            or not isinstance(digest, str)
            or not HEX_SHA256.fullmatch(digest)
            or type(size) is not int
            or size <= 0
        ):
            raise SourceVerificationError("unsafe or incomplete file metadata")
        if size > MAX_MEMBER_BYTES or total_bytes + size > MAX_TOTAL_BYTES:
            raise SourceVerificationError("declared compressed source exceeds byte budget")
        total_bytes += size
        if name.lower() in seen:
            raise SourceVerificationError("duplicate or case-colliding source member")
        seen.add(name.lower())
        declared.add(name)
        validated.append((name, digest, size))
    # Validate the entire manifest before hashing potentially large members.
    for name, digest, size in validated:
        path = directory / name
        if path.is_symlink() or not path.is_file():
            raise SourceVerificationError(f"missing or unsafe staged member: {name}")
        # Use the same no-follow file descriptor for fstat and SHA-256, so
        # a concurrent symlink/file swap cannot invalidate earlier path tests.
        flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
        flags |= getattr(os, "O_NONBLOCK", 0)  # reject replaced FIFOs without blocking
        try:
            descriptor = os.open(path, flags)
            with os.fdopen(descriptor, "rb") as handle:
                st = os.fstat(handle.fileno())
                if not stat.S_ISREG(st.st_mode) or st.st_nlink != 1:
                    raise SourceVerificationError(f"unsafe staged member type or hardlink: {name}")
                if st.st_size != size:
                    raise SourceVerificationError(f"byte length mismatch: {name}")
                actual = hashlib.sha256()
                read_bytes = 0
                for block in iter(lambda: handle.read(CHUNK), b""):
                    read_bytes += len(block)
                    if read_bytes > size:
                        raise SourceVerificationError(f"staged member changed during hashing: {name}")
                    actual.update(block)
                final = os.fstat(handle.fileno())
                if (
                    read_bytes != size
                    or final.st_dev != st.st_dev
                    or final.st_ino != st.st_ino
                    or final.st_size != st.st_size
                    or final.st_mtime_ns != st.st_mtime_ns
                    or final.st_ctime_ns != st.st_ctime_ns
                    or final.st_nlink != 1
                ):
                    raise SourceVerificationError(f"staged member changed during hashing: {name}")
        except OSError as error:
            raise SourceVerificationError(f"missing or unsafe staged member: {name}") from error
        if actual.hexdigest() != digest:
            raise SourceVerificationError(f"sha256 mismatch: {name}")
    # The complete staging directory must match the pinned manifest, not
    # merely contain its listed members. A private directory prevents untrusted
    # entry replacement while this final inventory check runs.
    with os.scandir(directory) as entries:
        for entry in entries:
            if entry.name not in declared or not entry.is_file(follow_symlinks=False):
                raise SourceVerificationError(f"undeclared or unsafe staged entry: {entry.name}")
    return len(members)


def _reject_duplicate_json_fields(pairs):
    """Reject ambiguous JSON objects before inspecting pinned source metadata."""
    result = {}
    for key, value in pairs:
        if key in result:
            raise SourceVerificationError(f"duplicate source manifest field: {key}")
        result[key] = value
    return result


def _reject_nonstandard_json_constant(value):
    """JSON NaN/Infinity extensions cannot appear in pinned source metadata."""
    raise SourceVerificationError(f"nonstandard JSON number: {value}")


def load_source_manifest(path):
    """Read one bounded, stable, regular manifest without following symlinks."""
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0)
    try:
        with os.fdopen(os.open(path, flags), "rb") as handle:
            before = os.fstat(handle.fileno())
            if (not stat.S_ISREG(before.st_mode) or before.st_nlink != 1
                    or not 0 < before.st_size <= MAX_MANIFEST_BYTES):
                raise SourceVerificationError("unsafe or oversized source manifest")
            data = handle.read(MAX_MANIFEST_BYTES + 1)
            after = os.fstat(handle.fileno())
            if (len(data) != before.st_size
                    or (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns,
                        before.st_ctime_ns, before.st_nlink)
                    != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns,
                        after.st_ctime_ns, after.st_nlink)):
                raise SourceVerificationError("source manifest changed during reading")
    except OSError as exc:
        raise SourceVerificationError("unsafe or unreadable source manifest") from exc
    try:
        result = json.loads(
            data.decode("utf-8"),
            object_pairs_hook=_reject_duplicate_json_fields,
            parse_constant=_reject_nonstandard_json_constant,
        )
    except (UnicodeError, json.JSONDecodeError, RecursionError) as exc:
        raise SourceVerificationError("malformed source manifest") from exc
    if not isinstance(result, dict):
        raise SourceVerificationError("source manifest must be a JSON object")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    metadata = load_source_manifest(args.manifest)
    verified = verify_source_bytes(metadata, args.directory)
    print(f"Verified {verified} staged source file(s). Upstream authenticity not established.")


if __name__ == "__main__":
    main()
