#!/usr/bin/env python3
"""Discover complete public Wikimedia current-content SHA-256 XML exports.

Read-only metadata discovery. The presence of official SHA256SUMS establishes
export completion per Wikimedia documentation, not publisher signatures,
verified downloaded bytes or ready-to-render/installed article records.
"""
import argparse
from datetime import datetime, timezone
import json
import re
from urllib.error import HTTPError
from urllib.request import HTTPRedirectHandler, Request, build_opener

HOST = "https://dumps.wikimedia.org"
DATASET = "mediawiki_content_current"
PROJECT = re.compile(r"[a-z0-9_]{1,64}\Z")
DATE = re.compile(r"20[0-9]{2}-(0[1-9]|1[0-2])-01\Z")
COMPONENT = re.compile(r"[A-Za-z0-9_-][A-Za-z0-9_.-]*\Z")
MAX_METADATA_BYTES = 8 * 1024 * 1024
MAX_MEMBERS = 100_000


class ExportDiscoveryError(ValueError):
    pass


class _NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, msg, headers, newurl):
        raise ExportDiscoveryError("official checksum metadata cannot redirect")


def _valid_identity(project, date):
    if not isinstance(project, str) or not PROJECT.fullmatch(project):
        raise ExportDiscoveryError("invalid Wikimedia project ID")
    if not isinstance(date, str) or not DATE.fullmatch(date):
        raise ExportDiscoveryError("invalid monthly current-content export date")
    try:
        datetime.strptime(date, "%Y-%m-%d")
    except ValueError as error:
        raise ExportDiscoveryError("invalid monthly export date") from error


def _safe_relative_path(value):
    if not isinstance(value, str) or not value or value.startswith("/"):
        return False
    if value.startswith("./"):
        value = value[2:]
    components = value.split("/")
    if any(not COMPONENT.fullmatch(c) or c.endswith(".") for c in components):
        return False
    if any(c.split(".")[0].upper() in {"CON", "PRN", "AUX", "NUL"} for c in components):
        return False
    if any(len(c.split(".")[0]) == 4
           and c.split(".")[0][:3].upper() in {"COM", "LPT"}
           and c.split(".")[0][3] in "123456789" for c in components):
        return False
    return bool(value)


def inventory_url(project, date):
    _valid_identity(project, date)
    return f"{HOST}/other/{DATASET}/{project}/{date}/xml/bzip2/SHA256SUMS"


def parse_checksums(text, project, date):
    """Return sorted SHA-256 inventory; malformed or incomplete input fails."""
    base = inventory_url(project, date).rsplit("/", 1)[0] + "/"
    if not isinstance(text, str) or len(text.encode("utf-8")) > MAX_METADATA_BYTES:
        raise ExportDiscoveryError("oversized or missing SHA256SUMS")
    entries = {}
    for line in text.splitlines():
        if not line.strip():
            continue
        match = re.fullmatch(r"([a-fA-F0-9]{64}) ([ *])(.+)", line)
        if match is None:
            raise ExportDiscoveryError("invalid SHA256SUMS line")
        digest, _marker, raw = match.groups()
        path = raw[2:] if raw.startswith("./") else raw
        if not _safe_relative_path(raw) or not path.endswith(".xml.bz2"):
            raise ExportDiscoveryError("unsafe or unexpected current-content member")
        name = path.rsplit("/", 1)[-1]
        if not name.startswith(f"{project}-{date}-"):
            raise ExportDiscoveryError("checksum member has mismatched wiki or export date")
        alias = path.lower()
        if alias in entries:
            raise ExportDiscoveryError("duplicate or case-colliding current-content member")
        entries[alias] = {"path": path, "sha256": digest.lower(), "url": base + path}
        if len(entries) > MAX_MEMBERS:
            raise ExportDiscoveryError("excessive current-content member inventory")
    if not entries:
        raise ExportDiscoveryError("empty current-content member inventory")
    return {
        "authority": "Wikimedia Foundation",
        "dataset": DATASET,
        "release_id": f"{DATASET}:{project}:{date}",
        "release_date": date,
        "project": project,
        "generation_id": date,
        "completed": True,
        "checksum_algorithm": "sha256",
        "checksum_inventory_url": inventory_url(project, date),
        "files": sorted(entries.values(), key=lambda entry: entry["path"]),
        "note": "Official HTTPS SHA-256 inventory, not a publisher signature or installed snapshot",
    }


def fetch_official(url):
    if not url.startswith(HOST + "/other/" + DATASET + "/"):
        raise ExportDiscoveryError("nonofficial checksum URL")
    client = build_opener(_NoRedirect())
    with client.open(Request(url, headers={
        "User-Agent": "FoundationWikipedia/0.1 current-content-discovery",
        "Accept-Encoding": "identity",
    }), timeout=25) as response:
        if response.geturl() != url or response.status != 200:
            raise ExportDiscoveryError("unexpected checksum response origin/status")
        data = response.read(MAX_METADATA_BYTES + 1)
    if len(data) > MAX_METADATA_BYTES:
        raise ExportDiscoveryError("oversized SHA256SUMS")
    return data.decode("utf-8")


def candidate_months(months, now=None):
    if type(months) is not int or not (1 <= months <= 24):
        raise ExportDiscoveryError("invalid monthly scan limit")
    now = now or datetime.now(timezone.utc)
    year, month = now.year, now.month
    found = []
    for _ in range(months):
        found.append(f"{year:04}-{month:02}-01")
        month -= 1
        if month == 0:
            month, year = 12, year - 1
    return found


def discover(project, dates, *, fetcher=fetch_official):
    if not isinstance(dates, (list, tuple)) or not dates:
        raise ExportDiscoveryError("missing candidate monthly dates")
    if not isinstance(project, str) or not PROJECT.fullmatch(project):
        raise ExportDiscoveryError("invalid Wikimedia project ID")
    for date in dates:
        url = inventory_url(project, date)
        try:
            return parse_checksums(fetcher(url), project, date)
        except HTTPError as error:
            if error.code != 404:
                raise
    raise ExportDiscoveryError("no completed public current-content export in scanned months")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("project", help="Wikimedia wiki ID, e.g. enwiki")
    parser.add_argument("--months", type=int, default=4)
    args = parser.parse_args()
    try:
        report = discover(args.project, candidate_months(args.months))
    except (ExportDiscoveryError, HTTPError, OSError, UnicodeError) as error:
        parser.exit(2, f"Current-content discovery failed: {error}\n")
    print(json.dumps(report, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
