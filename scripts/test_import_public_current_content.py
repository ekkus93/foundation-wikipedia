"""Regression tests for the official current-content XML fallback reader."""
import bz2
import hashlib
from pathlib import Path
import tempfile
import unittest

from import_public_current_content import PublicDumpError, iter_verified_pages


def page(page_id=42, revision=99, redirect=""):
    return (
        f"<page><title>Gravity</title><ns>0</ns><id>{page_id}</id>"
        f"{redirect}<revision><id>{revision}</id>"
        "<timestamp>2026-10-10T00:00:00Z</timestamp>"
        "<text xml:space='preserve'>Gravité 🌍</text></revision></page>"
    )


class PublicCurrentContentTests(unittest.TestCase):
    def read(self, xml, *, corrupt_digest=False):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "fixture.xml.bz2"
            data = bz2.compress(xml.encode("utf-8"))
            path.write_bytes(data)
            digest = hashlib.sha256(data).hexdigest()
            if corrupt_digest:
                digest = "0" * 64
            return list(iter_verified_pages(path, digest))

    def test_revision_unicode_and_redirect_survive(self):
        xml = "<mediawiki>" + page(42, 99, "<redirect title='Gravity (physics)'/>") + "</mediawiki>"
        result = self.read(xml)
        self.assertEqual(len(result), 1)
        self.assertEqual(result[0].page_id, 42)
        self.assertEqual(result[0].revision_id, 99)
        self.assertEqual(result[0].wikitext, "Gravité 🌍")
        self.assertEqual(result[0].redirect_title, "Gravity (physics)")

    def test_rejects_wrong_checksum_before_parsing(self):
        with self.assertRaisesRegex(PublicDumpError, "checksum mismatch"):
            self.read("<mediawiki/>", corrupt_digest=True)

    def test_duplicate_and_malformed_revision_fail(self):
        with self.assertRaisesRegex(PublicDumpError, "duplicate page"):
            self.read("<mediawiki>" + page() + page() + "</mediawiki>")
        with self.assertRaises(PublicDumpError):
            self.read("<mediawiki>" + page(revision=0) + "</mediawiki>")

    def test_rejects_dtd_and_wrong_root(self):
        with self.assertRaisesRegex(PublicDumpError, "unsafe or unrecognized"):
            self.read("<!DOCTYPE mediawiki><mediawiki>" + page() + "</mediawiki>")
        with self.assertRaisesRegex(PublicDumpError, "unsafe or unrecognized"):
            self.read("<notmediawiki>" + page() + "</notmediawiki>")

    def test_rejects_corrupt_bzip2_after_checksum(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "bad.xml.bz2"
            data = b"not a bzip2 stream"
            path.write_bytes(data)
            with self.assertRaisesRegex(PublicDumpError, "invalid bzip2"):
                list(iter_verified_pages(path, hashlib.sha256(data).hexdigest()))

    def test_rejects_duplicate_identity_fields(self):
        duplicate = page().replace("<id>42</id>", "<id>42</id><id>42</id>", 1)
        with self.assertRaisesRegex(PublicDumpError, "duplicate id"):
            self.read("<mediawiki>" + duplicate + "</mediawiki>")

    def test_rejects_out_of_range_canonical_identifiers(self):
        with self.assertRaisesRegex(PublicDumpError, "out-of-range"):
            self.read("<mediawiki>" + page(page_id=2 ** 64) + "</mediawiki>")
        bad_namespace = page().replace("<ns>0</ns>", "<ns>2147483648</ns>")
        with self.assertRaisesRegex(PublicDumpError, "out-of-range namespace"):
            self.read("<mediawiki>" + bad_namespace + "</mediawiki>")

    def test_rejects_truncated_xml(self):
        with self.assertRaisesRegex(PublicDumpError, "invalid or truncated"):
            self.read("<mediawiki>" + page())


if __name__ == "__main__":
    unittest.main()
