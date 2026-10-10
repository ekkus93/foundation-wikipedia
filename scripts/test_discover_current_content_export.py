"""Offline adversarial tests for official SHA-256 export discovery."""
from datetime import datetime, timezone
from urllib.error import HTTPError
import unittest

from discover_current_content_export import (
    ExportDiscoveryError, candidate_months, discover, inventory_url, parse_checksums,
)

PROJECT = "enwiki"
DATE = "2026-10-01"
NAME = "enwiki-2026-10-01-p123p456.xml.bz2"
SHA = "a" * 64


class CurrentContentExportTests(unittest.TestCase):
    def test_sha256_and_canonical_official_urls(self):
        report = parse_checksums(f"{SHA}  {NAME}\n", PROJECT, DATE)
        self.assertTrue(report["completed"])
        self.assertEqual(report["authority"], "Wikimedia Foundation")
        self.assertEqual(report["dataset"], "mediawiki_content_current")
        self.assertEqual(report["release_id"], "mediawiki_content_current:enwiki:2026-10-01")
        self.assertEqual(report["release_date"], DATE)
        self.assertEqual(report["checksum_algorithm"], "sha256")
        self.assertEqual(report["files"][0]["sha256"], SHA)
        self.assertEqual(
            report["files"][0]["url"],
            inventory_url(PROJECT, DATE).replace("SHA256SUMS", NAME),
        )

    def test_order_and_binary_mode_are_deterministic(self):
        other = "enwiki-2026-10-01-p457p999.xml.bz2"
        a = parse_checksums(f"{SHA} *{other}\n{'b' * 64}  ./{NAME}\n", PROJECT, DATE)
        b = parse_checksums(f"{'b' * 64}  {NAME}\n{SHA} *{other}\n", PROJECT, DATE)
        self.assertEqual(a, b)

    def test_invalid_inventory_fails_closed(self):
        for value in (
            "", "not-a-hash  article.xml.bz2",
            f"{SHA}  ../{NAME}", f"{SHA}  /tmp/{NAME}",
            f"{SHA}  otherwiki-2026-10-01-p1p2.xml.bz2",
            f"{SHA}  {NAME}?download=1", f"{SHA}  {NAME}\n{SHA}  {NAME}",
            f"{SHA}  COM1/{NAME}",
        ):
            with self.assertRaises(ExportDiscoveryError):
                parse_checksums(value, PROJECT, DATE)

    def test_project_month_limits_and_dates(self):
        for project in ("../enwiki", "ENWIKI", "enwiki/"):
            with self.assertRaises(ExportDiscoveryError):
                inventory_url(project, DATE)
        for date in ("2026-10-09", "2026-02-30", "2026/10/01", "2025-13-01"):
            with self.assertRaises(ExportDiscoveryError):
                inventory_url(PROJECT, date)
        self.assertEqual(
            candidate_months(3, datetime(2026, 1, 9, tzinfo=timezone.utc)),
            ["2026-01-01", "2025-12-01", "2025-11-01"],
        )

    def test_only_404_skips_an_unpublished_month(self):
        newer, older = "2026-10-01", "2026-09-01"
        attempted = []

        def fetch(url):
            attempted.append(url)
            if newer in url:
                raise HTTPError(url, 404, "not complete yet", {}, None)
            return f"{SHA}  enwiki-{older}-p1p3.xml.bz2\n"

        self.assertEqual(discover(PROJECT, [newer, older], fetcher=fetch)["generation_id"], older)
        self.assertEqual(len(attempted), 2)

        def failed(url):
            raise HTTPError(url, 503, "service unavailable", {}, None)

        with self.assertRaises(HTTPError):
            discover(PROJECT, [newer], fetcher=failed)


if __name__ == "__main__":
    unittest.main()
