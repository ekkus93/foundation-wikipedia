#!/usr/bin/env python3
"""Fail closed on malformed or non-AArch64 Android ELF shared objects."""
import argparse
import os
from pathlib import Path
import stat
import struct

ELF_MAGIC = b"\x7fELF"
MAX_NATIVE_BYTES = 256 * 1024 * 1024
MAX_PROGRAM_HEADERS = 1024


def verify_android_arm64_library(path):
    path = Path(path)
    if path.is_symlink() or not path.is_file():
        raise ValueError("native library missing or symlinked")
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    flags |= getattr(os, "O_NONBLOCK", 0)
    try:
        fd = os.open(path, flags)
        with os.fdopen(fd, "rb") as handle:
            metadata = os.fstat(handle.fileno())
            if (not stat.S_ISREG(metadata.st_mode)
                    or not 120 <= metadata.st_size <= MAX_NATIVE_BYTES):
                raise ValueError("missing or oversized native ELF payload")
            header = handle.read(64)
            if len(header) != 64 or header[:4] != ELF_MAGIC:
                raise ValueError("invalid ELF header")
            if tuple(header[4:7]) != (2, 1, 1):
                raise ValueError("expected 64-bit little-endian ELF")
            kind, machine, version = struct.unpack_from("<HHI", header, 16)
            if (kind, machine, version) != (3, 183, 1):
                raise ValueError("expected AArch64 shared object")
            phoffset = struct.unpack_from("<Q", header, 32)[0]
            header_size, phentry_size, phcount = struct.unpack_from("<HHH", header, 52)
            if (header_size != 64 or phentry_size != 56 or not 1 <= phcount <= MAX_PROGRAM_HEADERS
                    or phoffset < 64
                    or phoffset + phentry_size * phcount > metadata.st_size):
                raise ValueError("missing or out-of-bounds ELF program headers")
            handle.seek(phoffset)
            program_headers = handle.read(phentry_size * phcount)
            if len(program_headers) != phentry_size * phcount:
                raise ValueError("truncated ELF program headers")
            has_load = False
            for index in range(phcount):
                kind, _flags, offset, _vaddr, _paddr, filesz, memsz, _align = (
                    struct.unpack_from("<IIQQQQQQ", program_headers, index * phentry_size)
                )
                if offset > metadata.st_size or filesz > metadata.st_size - offset:
                    raise ValueError("out-of-bounds ELF segment")
                if filesz > memsz:
                    raise ValueError("ELF load segment exceeds memory allocation")
                if kind == 1 and filesz > 0:
                    has_load = True
            if not has_load:
                raise ValueError("ELF has no loadable file segment")
    except OSError as error:
        raise ValueError("unreadable or unsafe native library") from error
    return True


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("library")
    args = parser.parse_args()
    verify_android_arm64_library(args.library)
    print("Validated AArch64 shared-object ELF header and bounded segments")
