#!/usr/bin/env python3
"""Validate that a Rust native library is an AArch64 ELF shared object."""
import argparse
from pathlib import Path
import struct

ELF_MAGIC = bytes((127, 69, 76, 70))


def verify_android_arm64_library(path):
    path = Path(path)
    if path.is_symlink() or not path.is_file():
        raise ValueError("native library missing or symlinked")
    with path.open("rb") as handle:
        header = handle.read(64)
    if len(header) < 64 or header[:4] != ELF_MAGIC:
        raise ValueError("invalid ELF header")
    if tuple(header[4:7]) != (2, 1, 1):
        raise ValueError("expected 64-bit little-endian ELF")
    kind, machine, version = struct.unpack_from("<HHI", header, 16)
    if (kind, machine, version) != (3, 183, 1):
        raise ValueError("expected AArch64 shared object")
    return True


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("library")
    args = parser.parse_args()
    verify_android_arm64_library(args.library)
    print("Validated AArch64 ELF shared-object header")
