"""Synthetic ELF header tests; actual arm64 build is qualified by NDK CI."""
import struct
import tempfile
import unittest
from pathlib import Path

from verify_android_elf import verify_android_arm64_library


class AndroidElfTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.file = self.root / "libwiki_ffi.so"

    def header(self, machine=183, kind=3, endian=1):
        data = bytearray(64)
        data[:4] = b"\x7fELF"
        data[4:7] = bytes((2, endian, 1))
        struct.pack_into("<HHI", data, 16, kind, machine, 1)
        self.file.write_bytes(data)
        return self.file

    def test_valid_arm64_shared_object_header(self):
        self.assertTrue(verify_android_arm64_library(self.header()))

    def test_truncated_wrong_arch_wrong_kind_or_wrong_endianness(self):
        for options in (
            {"machine": 62}, {"kind": 2}, {"endian": 2},
        ):
            with self.subTest(options=options):
                with self.assertRaises(ValueError):
                    verify_android_arm64_library(self.header(**options))
        self.file.write_bytes(b"\x7fELF")
        with self.assertRaisesRegex(ValueError, "invalid ELF"):
            verify_android_arm64_library(self.file)

    def test_symlink_and_missing_library_are_rejected(self):
        self.header()
        symlink = self.root / "alias.so"
        symlink.symlink_to(self.file)
        with self.assertRaisesRegex(ValueError, "symlink"):
            verify_android_arm64_library(symlink)
        with self.assertRaisesRegex(ValueError, "missing"):
            verify_android_arm64_library(self.root / "absent.so")


if __name__ == "__main__":
    unittest.main()
