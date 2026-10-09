#!/usr/bin/env python3
"""Validate SHA-256 and source attribution of committed fixture files."""
import hashlib
import json
from pathlib import Path
import re
import sys

DIGEST = re.compile(r"^[0-9a-f]{64}$")


def verify(root):
    errors = []
    if not root.exists():
        return errors
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            errors.append(f"{path}: symlink not allowed")
            continue
        if not path.is_file() or path.name == "README.md" or path.name.endswith(".provenance.json"):
            continue
        sidecar = Path(str(path) + ".provenance.json")
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
        if data.get("source") not in ("synthetic", "wikimedia"):
            errors.append(f"{sidecar}: invalid source")
        if not isinstance(data.get("license"), str) or not data["license"].strip():
            errors.append(f"{sidecar}: missing license")
        digest = data.get("sha256")
        if not isinstance(digest, str) or not DIGEST.fullmatch(digest):
            errors.append(f"{sidecar}: invalid sha256")
        elif digest != hashlib.sha256(path.read_bytes()).hexdigest():
            errors.append(f"{sidecar}: sha256 mismatch")
        if data.get("source") == "synthetic" and not data.get("description"):
            errors.append(f"{sidecar}: missing description")
        if data.get("source") == "wikimedia":
            if not str(data.get("source_url", "")).startswith("https://"):
                errors.append(f"{sidecar}: missing HTTPS source_url")
            for key in ("project", "page_id", "revision_id"):
                if not data.get(key):
                    errors.append(f"{sidecar}: missing {key}")
    return errors


if __name__ == "__main__":
    root = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parents[1] / "fixtures"
    failures = verify(root)
    for failure in failures:
        print(failure, file=sys.stderr)
    if failures:
        sys.exit(1)
    print(f"Fixture provenance verified: {root}")
