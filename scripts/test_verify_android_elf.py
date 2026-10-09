"""Offline tests for AArch64 ELF header verification."""
import struct
import tempfile
from pathlib import Path
import unittest
from verify_android_elf import verify_android_arm64_library


class AndroidElfTests(unittest.TestCase):
    def test_valid_and_invalid_elf_headers(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "libwiki_ffi.so"
            header = bytearray(64)
            header[:7] = bytes((127, 69, 76, 70, 2, 1, 1))
            struct.pack_into("<HHI", header, 16, 3, 183, 1)
            path.write_bytes(header)
            self.assertTrue(verify_android_arm64_library(path))
            for kind, machine in ((2, 183), (3, 62)):
                struct.pack_into("<HHI", header, 16, kind, machine, 1)
                path.write_bytes(header)
                with self.assertRaises(ValueError):
                    verify_android_arm64_library(path)
            path.write_bytes(b"invalid")
            with self.assertRaises(ValueError):
                verify_android_arm64_library(path)


if __name__ == "__main__":
    unittest.main()
