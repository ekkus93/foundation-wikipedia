import unittest
from pathlib import Path
from tempfile import TemporaryDirectory
from verify_fixture_provenance import verify


class NegativeFixtureProvenanceTests(unittest.TestCase):
    def test_fixture_without_sidecar_is_rejected(self):
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'fixture.txt').write_text('test', encoding='utf-8')
            self.assertTrue(any('missing provenance sidecar' in error for error in verify(root)))
