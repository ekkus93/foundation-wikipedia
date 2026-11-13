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

    def test_hosted_https_dependency_licenses_are_approved(self):
        packages = [
            {"name": "ring", "version": "0.17.14", "license": "Apache-2.0 AND ISC"},
            {"name": "rustls", "version": "0.23.46", "license": "Apache-2.0 OR ISC OR MIT"},
            {"name": "walkdir", "version": "2.5.0", "license": "Unlicense/MIT"},
            {
                "name": "wasi",
                "version": "0.11.1+wasi-snapshot-preview1",
                "license": "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT",
            },
            {"name": "webpki-root-certs", "version": "1.0.12", "license": "CDLA-Permissive-2.0"},
        ]
        self.assertEqual(check(packages), [])

    def test_unlisted_licenses_still_fail_closed(self):
        packages = [
            {"name": "copyleft", "version": "1.0.0", "license": "GPL-3.0-only"},
            {"name": "unknown", "version": "1.0.0", "license": None},
            {"name": "near-miss", "version": "1.0.0", "license": "ISC AND GPL-3.0-only"},
        ]
        self.assertEqual(len(check(packages)), 3)

    def test_html_sanitizer_dependency_licenses_are_approved(self):
        packages = [
            {"name": "cssparser", "version": "0.38.0", "license": "MPL-2.0"},
            {"name": "dtoa-short", "version": "0.3.5", "license": "MPL-2.0"},
            {
                "name": "tinyvec",
                "version": "1.13.3",
                "license": "Zlib OR Apache-2.0 OR MIT",
            },
        ]
        self.assertEqual(check(packages), [])

    def test_portable_bzip2_backend_license_is_approved(self):
        packages = [
            {"name": "bzip2", "version": "0.6.1", "license": "MIT OR Apache-2.0"},
            {"name": "libbz2-rs-sys", "version": "0.2.1", "license": "bzip2-1.0.6"},
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
