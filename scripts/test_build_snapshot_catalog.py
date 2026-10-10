"""SQLite catalog staging and bounded random-frame verification regressions."""
import hashlib
import json
from pathlib import Path
import struct
import tempfile
import unittest

from build_snapshot_catalog import CatalogError, build, lookup

def frame(payload=b"compressed fixture"):
    header = b"FWREC001" + struct.pack("<HHBBHQQ", 1, 1, 1, 1, 0, 200, len(payload))
    encoded = header + payload
    return struct.pack("<Q", len(encoded)) + encoded

class CatalogTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.first = frame()
        self.second = frame(b"other bytes")
        (self.root / "articles.shard").write_bytes(self.first + self.second)
        self.manifest = self.root / "index.ndjson"
        self.db = self.root / "index.sqlite"
        self.entries = [
            {"project": "enwiki", "page_id": 42, "title": "Earth",
             "aliases": ["The blue planet"], "revision_id": 500, "wikidata_id": "Q2",
             "shard": "articles.shard", "offset": 0, "frame_bytes": len(self.first)},
            {"project": "enwiki", "page_id": 43, "title": "Planet Earth",
             "shard": "articles.shard", "offset": len(self.first),
             "frame_bytes": len(self.second)},
        ]

    def write_manifest(self):
        self.manifest.write_text("".join(json.dumps(item) + "\n" for item in self.entries))

    def test_sqlite_schema_matches_rust_catalog_contract(self):
        import sqlite3
        self.write_manifest()
        build(self.manifest, self.root, self.db)
        with sqlite3.connect(self.db) as conn:
            self.assertEqual(
                [row[1] for row in conn.execute("PRAGMA table_info(records)")],
                ["project", "page_id", "title", "revision_id", "wikidata_id",
                 "shard_name", "frame_offset", "frame_bytes", "frame_sha256"],
            )
            self.assertEqual(
                [row[1] for row in conn.execute("PRAGMA table_info(title_index)")],
                ["project", "normalized_title", "page_id"],
            )
            self.assertEqual(
                conn.execute(
                    "SELECT shard_name,frame_offset FROM records WHERE project=? AND page_id=?",
                    ("enwiki", 43),
                ).fetchone(),
                ("articles.shard", len(self.first)),
            )

    def test_random_read_and_tamper_detection(self):
        self.write_manifest()
        build(self.manifest, self.root, self.db)
        found = lookup(self.db, self.root, "enwiki", "the_blue_planet")
        self.assertEqual(found["page_id"], 42)
        self.assertEqual(found["revision_id"], 500)
        self.assertEqual(found["frame"], self.first)
        self.assertEqual(found["frame_sha256"], hashlib.sha256(self.first).hexdigest())
        self.assertEqual(lookup(self.db, self.root, "enwiki", "Planet Earth")["page_id"], 43)
        self.assertIsNone(lookup(self.db, self.root, "enwiki", "Mars"))
        corrupted = bytearray((self.root / "articles.shard").read_bytes())
        corrupted[-1] ^= 1
        (self.root / "articles.shard").write_bytes(corrupted)
        with self.assertRaisesRegex(CatalogError, "digest mismatch"):
            lookup(self.db, self.root, "enwiki", "Planet Earth")

    def test_atomic_duplicate_rollback_and_no_overwrite(self):
        self.entries[1]["title"] = "Earth"
        self.write_manifest()
        with self.assertRaises(Exception):
            build(self.manifest, self.root, self.db)
        self.assertFalse(self.db.exists())
        self.entries[1]["title"] = "Planet Earth"
        self.write_manifest()
        build(self.manifest, self.root, self.db)
        with self.assertRaisesRegex(CatalogError, "overwrite"):
            build(self.manifest, self.root, self.db)

    def test_path_escape_bad_offset_and_truncated_shard(self):
        for replacement in [
            {"shard": "../escape"}, {"offset": -1},
            {"frame_bytes": len(self.first) - 1}, {"offset": 2**63},
        ]:
            with self.subTest(replacement=replacement):
                self.entries[0].update(replacement)
                self.write_manifest()
                with self.assertRaises(Exception):
                    build(self.manifest, self.root, self.db)
                self.assertFalse(self.db.exists())
                self.entries[0] = {
                    "project": "enwiki", "page_id": 42, "title": "Earth",
                    "shard": "articles.shard", "offset": 0, "frame_bytes": len(self.first),
                }

    def test_symlink_and_missing_shard(self):
        self.entries[0]["shard"] = "linked.shard"
        (self.root / "linked.shard").symlink_to(self.root / "articles.shard")
        self.write_manifest()
        with self.assertRaisesRegex(CatalogError, "unsafe shard"):
            build(self.manifest, self.root, self.db)

if __name__ == "__main__":
    unittest.main()
