"""Offline tests for independent completed-publication metadata revalidation."""
import copy
import hashlib
import json
import unittest

from verify_public_dump_publication import PublicationError, verify_publication

PROJECT = "enwiki"
DATE = "20261009"
JOB = "articlesmultistreamdump"
ROOT = f"https://dumps.wikimedia.org/{PROJECT}/{DATE}/"
NAMES = ("enwiki-20261009-a.xml.bz2", "enwiki-20261009-b.xml.bz2")


class PublicationTests(unittest.TestCase):
    def setUp(self):
        self.status = json.dumps({"jobs": {
            JOB: {"status": "done", "files": {
                name: {"url": f"/{PROJECT}/{DATE}/{name}", "size": i + 2,
                       "sha1": hashlib.sha1(name.encode()).hexdigest()}
                for i, name in enumerate(NAMES)
            }},
            "other": {"status": "skipped"},
        }})
        self.report = {
            "project": PROJECT, "generation_id": DATE, "job": JOB,
            "status_url": ROOT + "dumpstatus.json", "completed": True,
            "files": [
                {"name": name, "url": ROOT + name, "bytes": i + 2,
                 "sha1": hashlib.sha1(name.encode()).hexdigest()}
                for i, name in enumerate(NAMES)
            ],
        }
        self.requests = []

    def fetch(self, url):
        self.requests.append(url)
        if url == ROOT + "dumpstatus.json":
            return self.status
        self.fail("unexpected upstream URL")

    def check(self):
        return verify_publication(self.report, fetcher=self.fetch)

    def test_accepts_exact_publication_without_second_checksum_fetch(self):
        self.assertEqual(len(self.check()["files"]), 2)
        self.assertEqual(self.requests, [ROOT + "dumpstatus.json"])
        self.report["files"].reverse()
        self.assertEqual(len(self.check()["files"]), 2)

    def test_rejects_forged_inventory_and_digests(self):
        for transform in (
            lambda r: r["files"].pop(),
            lambda r: r["files"].append(copy.deepcopy(r["files"][0])),
            lambda r: r["files"][0].update(bytes=999),
            lambda r: r["files"][0].update(sha1="0" * 40),
            lambda r: r["files"][0].update(url="https://mirror.invalid/a"),
            lambda r: r["files"][0].update(name="renamed.xml"),
        ):
            saved = copy.deepcopy(self.report)
            transform(self.report)
            with self.assertRaises(PublicationError):
                self.check()
            self.report = saved

    def test_status_unfinished_or_changed_fails_closed(self):
        saved = self.status
        payload = json.loads(saved)
        payload["jobs"]["other"]["status"] = "in-progress"
        self.status = json.dumps(payload)
        with self.assertRaisesRegex(PublicationError, "complete official"):
            self.check()
        self.status = saved

    def test_no_network_for_invalid_identity(self):
        for field, value in (("project", "../evil"), ("generation_id", "20260230"),
                             ("job", "unexpected"), ("status_url", "https://evil.test")):
            report = copy.deepcopy(self.report)
            report[field] = value
            with self.assertRaises(PublicationError):
                verify_publication(report, fetcher=lambda _: self.fail("unexpected network"))
        report = copy.deepcopy(self.report)
        del report["job"]
        with self.assertRaises(PublicationError):
            verify_publication(report, fetcher=lambda _: self.fail("unexpected network"))

    def test_missing_status_checksum_can_use_official_sha1s(self):
        payload = json.loads(self.status)
        for member in payload["jobs"][JOB]["files"].values():
            del member["sha1"]
        sums = "".join(f"{entry['sha1']}  {entry['name']}\n" for entry in self.report["files"])
        def fetch(url):
            self.requests.append(url)
            if url == ROOT + "dumpstatus.json":
                return json.dumps(payload)
            if url == ROOT + f"{PROJECT}-{DATE}-sha1sums.txt":
                return sums
            self.fail("unexpected upstream URL")
        self.assertEqual(len(verify_publication(self.report, fetcher=fetch)["files"]), 2)
        with self.assertRaises(PublicationError):
            verify_publication(self.report, fetcher=lambda url: json.dumps(payload))


if __name__ == "__main__":
    unittest.main()
