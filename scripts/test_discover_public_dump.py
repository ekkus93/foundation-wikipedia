"""Offline-only adversarial tests for official dump metadata discovery."""
import json
import unittest

from discover_public_dump import DiscoveryError, available_dates, discover, parse_status


def status(job="done", other="done", sha1=True, url=None):
    filename = "enwiki-20261001-pages-articles-multistream.xml.bz2"
    info = {
        "url": url or f"/enwiki/20261001/{filename}",
        "size": 2048,
    }
    if sha1:
        info["sha1"] = "a" * 40
    return json.dumps({
        "version": "0.8",
        "jobs": {
            "articlesmultistreamdump": {"status": job, "files": {filename: info}},
            "pagetable": {"status": other},
        },
    })


class PublicDumpDiscoveryTests(unittest.TestCase):
    def test_index_dates_are_actual_dates_not_urls_or_path_traversal(self):
        html = '<a href="20261001/">a</a><a href="20260230/">bad</a><a href="../">x</a><a href="https://evil.test/20260901/">bad</a><a href="20260920/">b</a>'
        self.assertEqual(available_dates(html), ["20261001", "20260920"])

    def test_complete_job_and_generation_are_required(self):
        report = parse_status(status(), "enwiki", "20261001", "articlesmultistreamdump")
        self.assertTrue(report["completed"])
        self.assertTrue(report["all_files_have_upstream_checksums"])
        self.assertEqual(report["files"][0]["bytes"], 2048)
        for body in (status(job="in-progress"), status(other="failed"), status(other="waiting")):
            with self.assertRaises(DiscoveryError):
                parse_status(body, "enwiki", "20261001", "articlesmultistreamdump")

    def test_rejects_nonexistent_generation_date_and_reserved_member(self):
        with self.assertRaises(DiscoveryError):
            parse_status(status(), "enwiki", "20260230", "articlesmultistreamdump")
        record = json.loads(status())
        files = record["jobs"]["articlesmultistreamdump"]["files"]
        metadata = next(iter(files.values()))
        record["jobs"]["articlesmultistreamdump"]["files"] = {
            "CON.txt": {**metadata, "url": "/enwiki/20261001/CON.txt"}
        }
        with self.assertRaises(DiscoveryError):
            parse_status(json.dumps(record), "enwiki", "20261001", "articlesmultistreamdump")

    def test_checksum_absence_is_reported_not_claimed_verified(self):
        report = parse_status(status(sha1=False), "enwiki", "20261001", "articlesmultistreamdump")
        self.assertFalse(report["all_files_have_upstream_checksums"])
        self.assertFalse(report["files"][0]["has_upstream_checksum"])

    def test_rejects_forged_paths_and_malformed_metadata(self):
        for body in (
            status(url="/frwiki/20261001/forged.xml"),
            status(url="https://evil.test/file.xml"),
            '{"jobs":[]}',
            '{"jobs":{"articlesmultistreamdump":{"status":"done","files":{}}}}',
        ):
            with self.assertRaises(DiscoveryError):
                parse_status(body, "enwiki", "20261001", "articlesmultistreamdump")

    def test_discovers_latest_completed_not_latest_started(self):
        base = "https://dumps.wikimedia.org/enwiki/"
        responses = {
            base: '<a href="20261020/">new</a><a href="20261001/">old</a>',
            base + "20261020/dumpstatus.json": status(other="in-progress"),
            base + "20261001/dumpstatus.json": status(),
        }
        report = discover("enwiki", fetcher=lambda url: responses[url])
        self.assertEqual(report["generation_id"], "20261001")
        with self.assertRaises(DiscoveryError):
            discover("enwiki", max_dates=1, fetcher=lambda url: responses[url])

    def test_missing_status_document_falls_back_to_older_generation(self):
        from urllib.error import HTTPError

        base = "https://dumps.wikimedia.org/enwiki/"
        responses = {
            base: '<a href="20261020/">new</a><a href="20261001/">old</a>',
            base + "20261001/dumpstatus.json": status(),
        }

        def fetch(url):
            if url.endswith("20261020/dumpstatus.json"):
                raise HTTPError(url, 404, "missing", {}, None)
            return responses[url]

        self.assertEqual(discover("enwiki", fetcher=fetch)["generation_id"], "20261001")

    def test_rejects_invalid_project_before_any_fetch(self):
        for project in ("../enwiki", "ENWIKI", "enwiki/", "https://evil.test"):
            with self.assertRaises(DiscoveryError):
                discover(project, fetcher=lambda _: self.fail("unexpected fetch"))


if __name__ == "__main__":
    unittest.main()
