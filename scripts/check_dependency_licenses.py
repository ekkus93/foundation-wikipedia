#!/usr/bin/env python3
"""Fail closed when a root Rust dependency has no approved SPDX license."""
import json
import subprocess
import sys

ALLOWED = {
    "MIT", "Apache-2.0", "MIT OR Apache-2.0",
    "Apache-2.0 OR MIT", "BSD-3-Clause", "ISC",
    "Unicode-3.0", "Zlib",
}


def check(packages):
    return [
        f"{pkg['name']} {pkg['version']}: unapproved license {pkg.get('license')!r}"
        for pkg in packages
        if pkg.get("license") not in ALLOWED
    ]


if __name__ == "__main__":
    result = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked"],
        capture_output=True, text=True, check=True,
    )
    packages = json.loads(result.stdout)["packages"]
    errors = check(packages)
    for error in errors:
        print(error, file=sys.stderr)
    if errors:
        sys.exit(1)
    print(f"Rust dependency licenses verified: {len(packages)} packages")
