import unittest
from pathlib import Path
from verify_fixture_provenance import verify


class ProvenanceTests(unittest.TestCase):
    def test_committed_fixtures(self):
        root = Path(__file__).resolve().parents[1] / 'fixtures'
        self.assertEqual(verify(root), [])
