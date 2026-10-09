#!/usr/bin/env python3
"""Discover completed public Wikimedia dump generations from official status JSON.

This is a read-only metadata discovery helper, NOT a downloader, source
authenticator, SHA-256 manifest generator, or article renderer.
"""
import argparse
from datetime import datetime
from html.parser import HTMLParser
import json
import re
from urllib.error import HTTPError
from urllib.request import Request, build_opener, HTTPRedirectHandler

HOST = "https://dumps.wikimedia.org"
PROJECT = re.compile(r"^[a-z0-9_]+$")
DATE = re.compile(r"^[0-9]{8}$")
FILENAME = re.compile(r"^[A-Za-z0-9_-][A-Za-z0-9_.-]*$")
HEX_SHA1 = re.compile(r"^[0-9a-fA-F]{40}$")
HEX_MD5 = re.compile(r"^[0-9a-fA-F]{32}$")
MAX_BYTES = 8 * 1024 * 1024


class DiscoveryError(ValueError):
    pass


class _NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, msg, headers, newurl):
        raise DiscoveryError("redirect rejected for official source metadata")


class _Dates(HTMLParser):
    def __init__(self):
        super().__init__()
        self.values = set()

    def handle_starttag(self, tag, attrs):
        if tag != "a":
            return
        for name, value in attrs:
            if name == "href" and isinstance(value, str) and DATE.fullmatch(value.rstrip("/")) and value.endswith("/"):
                try:
                    datetime.strptime(value[:-1], "%Y%m%d")
                except ValueError:
                    continue
                self.values.add(value[:-1])


def available_dates(html):
    parser = _Dates()
    parser.feed(html)
    return sorted(parser.values, reverse=True)


def parse_status(payload, project, generation, job_name):
    """Accept only a fully terminal generation and a completed requested job."""
    if not PROJECT.fullmatch(project) or not DATE.fullmatch(generation):
        raise DiscoveryError("invalid project or generation")
    try:
        status = json.loads(payload)
    except (ValueError, TypeError) as error:
        raise DiscoveryError("invalid dump status JSON") from error
    if not isinstance(status, dict) or not isinstance(status.get("jobs"), dict):
        raise DiscoveryError("missing dump jobs")
    jobs = status["jobs"]
    if not jobs or any(
        not isinstance(job, dict) or job.get("status") not in {"done", "skipped"}
        for job in jobs.values()
    ):
        raise DiscoveryError("generation has unfinished or failed jobs")
    job = jobs.get(job_name)
    if not isinstance(job, dict) or job.get("status") != "done":
        raise DiscoveryError("requested dump job not completed")
    entries = job.get("files")
    if not isinstance(entries, dict) or not entries or len(entries) > 100000:
        raise DiscoveryError("missing or excessive dump file entries")
    files = []
    seen = set()
    for name, info in sorted(entries.items()):
        if not isinstance(name, str) or not FILENAME.fullmatch(name) or name.endswith("."):
            raise DiscoveryError("unsafe dump filename")
        if name.lower() in seen:
            raise DiscoveryError("case-colliding dump filenames")
        seen.add(name.lower())
        if not isinstance(info, dict):
            raise DiscoveryError("invalid dump file metadata")
        expected_path = f"/{project}/{generation}/{name}"
        size = info.get("size")
        if info.get("url") != expected_path or type(size) is not int or size <= 0:
            raise DiscoveryError("unexpected official dump file path or size")
        sha1, md5 = info.get("sha1"), info.get("md5")
        if sha1 is not None and (not isinstance(sha1, str) or not HEX_SHA1.fullmatch(sha1)):
            raise DiscoveryError("malformed upstream SHA-1")
        if md5 is not None and (not isinstance(md5, str) or not HEX_MD5.fullmatch(md5)):
            raise DiscoveryError("malformed upstream MD5")
        files.append({
            "name": name,
            "url": HOST + expected_path,
            "bytes": size,
            "sha1": sha1,
            "md5": md5,
            "has_upstream_checksum": bool(sha1 or md5),
        })
    return {
        "project": project,
        "generation_id": generation,
        "job": job_name,
        "completed": True,
        "status_url": f"{HOST}/{project}/{generation}/dumpstatus.json",
        "files": files,
        "all_files_have_upstream_checksums": all(item["has_upstream_checksum"] for item in files),
        "warning": "Official HTTPS metadata only; upstream SHA-1/MD5 are not SHA-256 or a signed publisher manifest.",
    }


def fetch_official(url):
    if not url.startswith(HOST + "/"):
        raise DiscoveryError("nonofficial source URL")
    opener = build_opener(_NoRedirect())
    with opener.open(Request(url, headers={"User-Agent": "FoundationWikipedia/0.1 source-discovery"}), timeout=20) as response:
        if response.geturl() != url:
            raise DiscoveryError("unexpected source URL after fetch")
        data = response.read(MAX_BYTES + 1)
    if len(data) > MAX_BYTES:
        raise DiscoveryError("oversized source metadata")
    return data.decode("utf-8")


def discover(project, job_name="articlesmultistreamdump", max_dates=8, fetcher=fetch_official):
    if not PROJECT.fullmatch(project) or not re.fullmatch(r"[a-z0-9_]+", job_name):
        raise DiscoveryError("invalid project or job")
    if not 1 <= max_dates <= 24:
        raise DiscoveryError("invalid date scan limit")
    index = fetcher(f"{HOST}/{project}/")
    for generation in available_dates(index)[:max_dates]:
        try:
            status = fetcher(f"{HOST}/{project}/{generation}/dumpstatus.json")
            return parse_status(status, project, generation, job_name)
        except DiscoveryError:
            continue
        except HTTPError as error:
            if error.code != 404:
                raise
            continue
    raise DiscoveryError("no fully completed matching dump generation in scanned dates")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("project", help="e.g. enwiki")
    parser.add_argument("--job", default="articlesmultistreamdump")
    parser.add_argument("--max-dates", type=int, default=8)
    args = parser.parse_args()
    try:
        report = discover(args.project, args.job, args.max_dates)
    except (DiscoveryError, OSError, UnicodeError) as error:
        parser.exit(2, f"Discovery failed: {error}\n")
    print(json.dumps(report, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
