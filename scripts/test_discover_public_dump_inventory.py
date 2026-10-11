"""Offline discovery fixtures for official current-content publications."""
from pathlib import Path
import hashlib
import io
import tempfile
import unittest
from urllib.error import HTTPError

from discover_public_dump_inventory import (
    BASE, PublicationError, _official_url, discover_completed_export,
    save_inventory, download_verified_shard,
)


def inventory(date):
    return (("a" * 64) + "  enwiki-" + date + "-p1p100.xml.bz2\n").encode()


class DiscoveryTests(unittest.TestCase):
    def test_discovers_newest_completed_publication_without_guessing_date(self):
        index = BASE + "enwiki/"
        html = b'<a href="../">Parent</a><a href="2026-10-01/">Oct</a><a href="2026-09-01/">Sep</a>'
        visited = []

        def fetch(url, limit):
            visited.append(url)
            if url == index:
                return html
            if url.endswith("/2026-10-01/xml/bzip2/SHA256SUMS"):
                raise HTTPError(url, 404, "incomplete", {}, None)
            if url.endswith("/2026-09-01/xml/bzip2/SHA256SUMS"):
                return inventory("2026-09-01")
            raise AssertionError(url)

        metadata, data = discover_completed_export("enwiki", fetch)
        self.assertEqual(metadata["date"], "2026-09-01")
        self.assertEqual(metadata["member_count"], 1)
        self.assertEqual(data, inventory("2026-09-01"))
        self.assertEqual(len(visited), 3)

    def test_rejects_invalid_dates_and_absent_publications(self):
        def fetch(url, limit):
            return b'<a href="2026-99-01/">Invalid</a><a href="2026-02-01/">Feb</a>'
        with self.assertRaisesRegex(PublicationError, "no completed"):
            discover_completed_export("enwiki", lambda url, limit:
                                      fetch(url, limit) if url.endswith("enwiki/")
                                      else (_ for _ in ()).throw(
                                          HTTPError(url, 404, "missing", {}, None)))
        with self.assertRaisesRegex(PublicationError, "invalid wiki project"):
            discover_completed_export("../enwiki", fetch)
        with self.assertRaisesRegex(PublicationError, "invalid wiki project"):
            discover_completed_export("e" * 256, fetch)

    def test_rejects_untrusted_origin_and_invalid_checksum_inventory(self):
        for url in (
            "http://dumps.wikimedia.org/other/mediawiki_content_current/enwiki/",
            "https://evil.example/other/mediawiki_content_current/enwiki/",
            "https://dumps.wikimedia.org.evil.example/other/mediawiki_content_current/",
            "https://dumps.wikimedia.org/other/mediawiki_content_current/?x=1",
        ):
            with self.assertRaisesRegex(PublicationError, "origin"):
                _official_url(url)

        index = BASE + "enwiki/"
        def bad_fetch(url, limit):
            if url == index:
                return b'<a href="2026-10-01/">Oct</a>'
            return b"not a checksum"
        with self.assertRaisesRegex(PublicationError, "inventory member"):
            discover_completed_export("enwiki", bad_fetch)

    def test_inventory_rejects_overlapping_partitions_and_unsafe_paths(self):
        date = "2026-10-01"
        prefix = f"enwiki-{date}-p"

        def check(paths):
            data = "".join(f"{'a' * 64}  {path}\n" for path in paths).encode()

            def fetch(url, limit):
                if url.endswith("enwiki/"):
                    return b'<a href="2026-10-01/">Oct</a>'
                if url.endswith("SHA256SUMS"):
                    return data
                raise AssertionError(url)

            return discover_completed_export("enwiki", fetch)

        adjacent = [f"{prefix}1p100.xml.bz2", f"{prefix}101p200.xml.bz2"]
        self.assertEqual(check(adjacent)[0]["member_count"], 2)
        # Gaps are legitimate when IDs were deleted or never assigned.
        self.assertEqual(check([adjacent[0], f"{prefix}201p300.xml.bz2"])[0]
                         ["member_count"], 2)
        for members in (
            [adjacent[0], f"{prefix}100p200.xml.bz2"],
            [adjacent[0], f"other/{prefix}1p100.xml.bz2"],
            [adjacent[0], f"{prefix}50p150.xml.bz2"],
        ):
            with self.subTest(members=members):
                with self.assertRaisesRegex(PublicationError, "overlapping"):
                    check(members)
        for path in (
            f"CON/{prefix}1p100.xml.bz2",
            f"nested/COM1/{prefix}1p100.xml.bz2",
            f"{'a' * 256}/{prefix}1p100.xml.bz2",
            f"{prefix}0p100.xml.bz2",
            f"{prefix}1p18446744073709551616.xml.bz2",
        ):
            with self.subTest(path=path):
                with self.assertRaises(PublicationError):
                    check([path])

    def test_downloads_only_inventory_listed_hash_verified_shard(self):
        payload = b"compressed synthetic shard bytes"
        digest = hashlib.sha256(payload).hexdigest()
        member = "enwiki-2026-10-01-p1p100.xml.bz2"
        listed = (digest + "  " + member + "\n").encode()

        def fetch(url, limit):
            if url.endswith("enwiki/"):
                return b'<a href="2026-10-01/">Oct</a>'
            if url.endswith("SHA256SUMS"):
                return listed
            raise AssertionError(url)

        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / member
            def stream(url):
                self.assertTrue(url.endswith(member))
                return io.BytesIO(payload)

            receipt = download_verified_shard("enwiki", member, output,
                                              fetch=fetch, stream=stream)
            self.assertEqual(output.read_bytes(), payload)
            self.assertEqual(receipt["sha256"], digest)
            self.assertEqual(receipt["bytes"], len(payload))
            with self.assertRaisesRegex(PublicationError, "already exists"):
                download_verified_shard("enwiki", member, output,
                                        fetch=fetch, stream=stream)
            output.unlink()
            with self.assertRaisesRegex(PublicationError, "absent"):
                download_verified_shard("enwiki", "other.xml.bz2", output,
                                        fetch=fetch, stream=stream)
            with self.assertRaisesRegex(PublicationError, "SHA-256 mismatch"):
                download_verified_shard("enwiki", member, output, fetch=fetch,
                                        stream=lambda url: io.BytesIO(b"tampered"))
            with self.assertRaisesRegex(PublicationError, "exceeds budget"):
                download_verified_shard("enwiki", member, output, fetch=fetch,
                                        stream=stream, max_compressed_bytes=8)
            self.assertFalse(output.exists())
            self.assertFalse(list(Path(directory).glob(".dump-shard.*")))

    def test_staging_is_atomic_and_never_overwrites(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "SHA256SUMS"
            save_inventory(output, inventory("2026-10-01"))
            self.assertEqual(output.read_bytes(), inventory("2026-10-01"))
            with self.assertRaisesRegex(PublicationError, "already exists"):
                save_inventory(output, b"changed")
            self.assertEqual(output.read_bytes(), inventory("2026-10-01"))
            self.assertFalse(list(Path(directory).glob(".dump-inventory.*")))


if __name__ == "__main__":
    unittest.main()
