#!/usr/bin/env python3
"""Fail-closed validation of committed fixture attribution and exact bytes."""
import hashlib
import json
from pathlib import Path
import re
import sys
from urllib.parse import urlsplit

DIGEST = re.compile(r"^[0-9a-f]{64}$")
PROJECT = re.compile(r"^[a-z0-9_]+$")
SUFFIX = ".provenance.json"


def _positive_id(value):
    return type(value) is int and value > 0


def _official_url(value):
    if not isinstance(value, str):
        return False
    try:
        parsed = urlsplit(value)
        return (
            parsed.scheme == "https"
            and bool(parsed.hostname)
            and parsed.username is None
            and parsed.password is None
        )
    except ValueError:
        return False


def verify(root):
    """Return all validation failures; never silently accept missing fixtures."""
    root = Path(root)
    if root.is_symlink() or not root.is_dir():
        return [f"{root}: missing or unsafe fixture directory"]

    errors = []
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            errors.append(f"{path}: symlink not allowed")
            continue
        if path.is_dir():
            continue
        if not path.is_file():
            errors.append(f"{path}: unsupported fixture entry")
            continue
        if path.name.endswith(SUFFIX):
            fixture = path.with_name(path.name[:-len(SUFFIX)])
            if not fixture.is_file() or fixture.is_symlink():
                errors.append(f"{path}: orphan provenance sidecar")
            continue
        if path.name == "README.md":
            continue

        sidecar = Path(str(path) + SUFFIX)
        if not sidecar.is_file() or sidecar.is_symlink():
            errors.append(f"{path}: missing provenance sidecar")
            continue
        try:
            data = json.loads(sidecar.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            errors.append(f"{sidecar}: invalid JSON")
            continue
        if not isinstance(data, dict):
            errors.append(f"{sidecar}: expected object")
            continue

        source = data.get("source")
        if source not in ("synthetic", "wikimedia"):
            errors.append(f"{sidecar}: invalid source")
        if not isinstance(data.get("license"), str) or not data["license"].strip():
            errors.append(f"{sidecar}: missing license")
        digest = data.get("sha256")
        if not isinstance(digest, str) or not DIGEST.fullmatch(digest):
            errors.append(f"{sidecar}: invalid sha256")
        else:
            try:
                actual = hashlib.sha256(path.read_bytes()).hexdigest()
            except OSError:
                errors.append(f"{path}: unable to read fixture")
            else:
                if digest != actual:
                    errors.append(f"{sidecar}: sha256 mismatch")

        if source == "synthetic":
            if not isinstance(data.get("description"), str) or not data["description"].strip():
                errors.append(f"{sidecar}: missing description")
        elif source == "wikimedia":
            if not _official_url(data.get("source_url")):
                errors.append(f"{sidecar}: missing HTTPS source_url")
            if not isinstance(data.get("project"), str) or not PROJECT.fullmatch(data["project"]):
                errors.append(f"{sidecar}: invalid project")
            for key in ("page_id", "revision_id"):
                if not _positive_id(data.get(key)):
                    errors.append(f"{sidecar}: invalid {key}")
    return errors


if __name__ == "__main__":
    root = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parents[1] / "fixtures"
    failures = verify(root)
    for failure in failures:
        print(failure, file=sys.stderr)
    if failures:
        sys.exit(1)
    print(f"Fixture provenance verified: {root}")
