#!/usr/bin/env python3
"""Fail-closed verification of a *local staging copy* against declared checksums.

Upstream authority and completeness must be established independently: a JSON
manifest written by an attacker is not proof of Wikimedia publication.
"""
import argparse
import hashlib
import json
import re
import stat
from pathlib import Path
from urllib.parse import urlsplit

HEX_SHA256 = re.compile(r"^[0-9a-f]{64}$")
IDENTITY = re.compile(r"^[a-z0-9_]+$")
NAME = re.compile(r"^[a-zA-Z0-9_-][a-zA-Z0-9_.-]*$")
GENERATION = re.compile(r"^[a-zA-Z0-9_-][a-zA-Z0-9_.-]*$")
MAX_MEMBERS = 100000
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
            and not any(char.isspace() or ord(char) < 32 or char == "\\\\" for char in value)
            and (
                (url.hostname == "dumps.wikimedia.org"
                 and url.path == f"/{project}/{generation}/dumpstatus.json")
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
    if directory.is_symlink() or not directory.is_dir():
        raise SourceVerificationError("staging directory is missing or unsafe")
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
        if name.lower() in seen:
            raise SourceVerificationError("duplicate or case-colliding source member")
        seen.add(name.lower())
        path = directory / name
        if path.is_symlink() or not path.is_file():
            raise SourceVerificationError(f"missing or unsafe staged member: {name}")
        st = path.stat()
        if not stat.S_ISREG(st.st_mode) or st.st_size != size:
            raise SourceVerificationError(f"byte length mismatch: {name}")
        actual = hashlib.sha256()
        with path.open("rb") as handle:
            for block in iter(lambda: handle.read(CHUNK), b""):
                actual.update(block)
        if actual.hexdigest() != digest:
            raise SourceVerificationError(f"sha256 mismatch: {name}")
    return len(members)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    metadata = json.loads(args.manifest.read_text(encoding="utf-8"))
    verified = verify_source_bytes(metadata, args.directory)
    print(f"Verified {verified} staged source file(s). Upstream authenticity not established.")


if __name__ == "__main__":
    main()
