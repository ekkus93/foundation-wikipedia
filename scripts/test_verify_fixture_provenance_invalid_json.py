import unittest
from pathlib import Path
from tempfile import TemporaryDirectory
from verify_fixture_provenance import verify


class InvalidJsonFixtureTest(unittest.TestCase):
    def test_invalid_json(self):
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'sample.txt').write_text('data')
            (root / 'sample.txt.provenance.json').write_text('{')
            self.assertTrue(any('invalid JSON' in e for e in verify(root)))
