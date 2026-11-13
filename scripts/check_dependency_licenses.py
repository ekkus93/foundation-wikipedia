#!/usr/bin/env python3
"""Fail closed when a root Rust dependency has no approved SPDX license."""
import json
import subprocess
import sys

ALLOWED = {
    "MIT",
    "Apache-2.0",
    "MIT OR Apache-2.0",
    "MIT/Apache-2.0",  # Legacy dual-license metadata from version_check 0.9.5
    "Apache-2.0 OR MIT",
    "BSD-3-Clause",
    "ISC",
    "Unicode-3.0",
    "Zlib",
    "Unlicense OR MIT",
    "(MIT OR Apache-2.0) AND Unicode-3.0",
    "MIT OR Apache-2.0 OR LGPL-2.1-or-later",
    "BSD-2-Clause OR Apache-2.0 OR MIT",
    "MPL-2.0",
    "Zlib OR Apache-2.0 OR MIT",
    # SPDX bzip2-1.0.6: permissive redistribution with retained notice;
    # see https://spdx.org/licenses/bzip2-1.0.6.html
    "bzip2-1.0.6",
    # AI-003 hosted HTTPS (rustls + OS certificate verification). The first
    # four are combinations of licenses already allowed individually.
    "Apache-2.0 AND ISC",  # ring
    "Apache-2.0 OR ISC OR MIT",  # rustls, rustls-native-certs
    "Unlicense/MIT",  # Legacy spelling of "Unlicense OR MIT" (walkdir, same-file)
    "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT",  # wasi
    # Owner-approved 2026-11-13: permissive data license for Mozilla's root
    # certificate list (webpki-root-certs). Only a wasm32 dependency of
    # rustls-platform-verifier, never compiled into the desktop/Android apps;
    # see https://spdx.org/licenses/CDLA-Permissive-2.0.html
    "CDLA-Permissive-2.0",
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
