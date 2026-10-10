"""Atomic public-dump staging regression."""
import bz2
import hashlib
from pathlib import Path
import tempfile
import unittest

from import_public_current_content import PublicDumpError
from stage_public_current_content import stage_verified_pages

PAGE = ("<page><title>Gravity</title><ns>0</ns><id>42</id>"
        "<revision><id>99</id><timestamp>2026-10-10</timestamp>"
        "<text>Gravity</text></revision></page>")


class StageTests(unittest.TestCase):
    def test_complete_and_incomplete_sources(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "source.bz2"
            target = root / "pages.ndjson"
            data = bz2.compress(("<mediawiki>" + PAGE + "</mediawiki>").encode())
            source.write_bytes(data)
            digest = hashlib.sha256(data).hexdigest()
            self.assertEqual(stage_verified_pages(source, digest, target), 1)
            self.assertIn("Gravity", target.read_text())
            target.unlink()
            data = bz2.compress(("<mediawiki>" + PAGE).encode())
            source.write_bytes(data)
            digest = hashlib.sha256(data).hexdigest()
            with self.assertRaises(PublicDumpError):
                stage_verified_pages(source, digest, target)
            self.assertFalse(target.exists())
