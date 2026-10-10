"""Regression tests for the root Rust dependency license gate."""
import json
import subprocess
import unittest

from check_dependency_licenses import check


class LicenseAuditTests(unittest.TestCase):
    def test_current_rust_dependency_graph_is_approved(self):
        output = subprocess.check_output(
            ["cargo", "metadata", "--format-version", "1", "--locked"],
            text=True,
        )
        packages = json.loads(output)["packages"]
        self.assertTrue(packages)
        self.assertEqual(check(packages), [])

    def test_known_composite_spdx_expressions_are_approved(self):
        packages = [
            {"name": "memchr", "version": "2.8.3", "license": "Unlicense OR MIT"},
            {
                "name": "unicode-ident",
                "version": "1.0.26",
                "license": "(MIT OR Apache-2.0) AND Unicode-3.0",
            },
        ]
        self.assertEqual(check(packages), [])

    def test_codec_dependency_composite_spdx_expressions_are_approved(self):
        packages = [
            {
                "name": "r-efi",
                "version": "6.0.0",
                "license": "MIT OR Apache-2.0 OR LGPL-2.1-or-later",
            },
            {
                "name": "zerocopy",
                "version": "0.8.62",
                "license": "BSD-2-Clause OR Apache-2.0 OR MIT",
            },
            {
                "name": "zerocopy-derive",
                "version": "0.8.62",
                "license": "BSD-2-Clause OR Apache-2.0 OR MIT",
            },
        ]
        self.assertEqual(check(packages), [])

    def test_missing_or_unapproved_license_fails(self):
        packages = [
            {"name": "missing", "version": "1.0", "license": None},
            {"name": "unknown", "version": "2.0", "license": "Unapproved"},
        ]
        self.assertEqual(len(check(packages)), 2)


if __name__ == "__main__":
    unittest.main()
