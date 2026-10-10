import sqlite3
import unittest
from pathlib import Path

SCHEMA = (Path(__file__).resolve().parents[1] /
          "crates/wiki-store/sql/snapshot_catalog.sql").read_text()
HASH = "a" * 64


class CatalogSchemaTests(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(":memory:")
        self.db.executescript(SCHEMA)
        self.db.execute("PRAGMA foreign_keys = ON")
        self.db.execute(
            "INSERT INTO shards VALUES(1,?,?,?)",
            ("articles.shard", HASH, 4096),
        )

    def add_page(self, length=100, revision=42):
        self.db.execute(
            "INSERT INTO pages VALUES(?,?,?,?,?,?,?,?,?,?)",
            ("enwiki", 7, revision, "article", "Physics",
             "Q413", 1, 0, length, HASH),
        )

    def test_version_and_lookup(self):
        self.assertEqual(self.db.execute("PRAGMA user_version").fetchone()[0], 1)
        self.add_page()
        self.assertEqual(
            self.db.execute("SELECT frame_bytes FROM pages WHERE "
                            "project=? AND title=? COLLATE NOCASE",
                            ("enwiki", "physics")).fetchone()[0], 100
        )

    def test_rejects_invalid_frame_and_revision(self):
        with self.assertRaises(sqlite3.IntegrityError):
            self.add_page(length=33554473)
        with self.assertRaises(sqlite3.IntegrityError):
            self.add_page(revision=0)

    def test_unique_shard_offset(self):
        self.add_page()
        with self.assertRaises(sqlite3.IntegrityError):
            self.add_page(revision=43)


if __name__ == "__main__":
    unittest.main()
