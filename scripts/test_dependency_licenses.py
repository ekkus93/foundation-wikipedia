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

    def test_missing_or_unapproved_license_fails(self):
        packages = [
            {"name": "missing", "version": "1.0", "license": None},
            {"name": "unknown", "version": "2.0", "license": "Unapproved"},
        ]
        self.assertEqual(len(check(packages)), 2)


if __name__ == "__main__":
    unittest.main()
