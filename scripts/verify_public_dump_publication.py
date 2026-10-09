#!/usr/bin/env python3
"""Revalidate a dump-discovery report against fresh official HTTPS metadata.

This binds a caller-supplied inventory to the publisher's *current* status
response; it is not a cryptographic signature on a historical publication.
"""
import argparse
from datetime import datetime
import json
from urllib.error import HTTPError

from discover_public_dump import (
    DATE, PROJECT, DiscoveryError, HOST, attach_official_sha1s, fetch_official, parse_status,
)


class PublicationError(ValueError):
    """The report does not match the official completed publication."""


def verify_publication(report, *, fetcher=fetch_official):
    """Return revalidated inventory; never trust caller-provided digests alone."""
    if not isinstance(report, dict) or report.get("completed") is not True:
        raise PublicationError("invalid or incomplete report")
    project = report.get("project")
    generation = report.get("generation_id")
    job = report.get("job")
    if not all(isinstance(value, str) for value in (project, generation, job)):
        raise PublicationError("missing publication identity")
    if job != "articlesmultistreamdump":
        raise PublicationError("unsupported dump job")
    status_url = f"{HOST}/{project}/{generation}/dumpstatus.json"
    if report.get("status_url") != status_url:
        raise PublicationError("invalid publication status URL")
    if not PROJECT.fullmatch(project) or not DATE.fullmatch(generation):
        raise PublicationError("invalid publication identity")
    try:
        datetime.strptime(generation, "%Y%m%d")
    except ValueError as error:
        raise PublicationError("invalid publication date") from error
    try:
        fresh = parse_status(fetcher(status_url), project, generation, job)
        if any(entry["sha1"] is None for entry in fresh["files"]):
            sums_url = f"{HOST}/{project}/{generation}/{project}-{generation}-sha1sums.txt"
            fresh = attach_official_sha1s(fresh, fetcher(sums_url))
    except (DiscoveryError, HTTPError, OSError, UnicodeError, ValueError) as error:
        raise PublicationError("failed to verify complete official publication") from error
    if not fresh["all_files_have_upstream_checksums"]:
        raise PublicationError("publication lacks required upstream SHA-1 checksums")
    files = report.get("files")
    if not isinstance(files, list) or len(files) != len(fresh["files"]):
        raise PublicationError("report member inventory differs from publication")
    expected = {
        entry["name"]: (entry["url"], entry["bytes"], entry["sha1"].lower())
        for entry in fresh["files"]
    }
    seen = set()
    for entry in files:
        if not isinstance(entry, dict) or not isinstance(entry.get("name"), str):
            raise PublicationError("invalid report member")
        name = entry["name"]
        if name in seen or name not in expected:
            raise PublicationError("report has duplicate or unknown member")
        seen.add(name)
        sha1 = entry.get("sha1")
        if not isinstance(sha1, str) or (
            entry.get("url"), entry.get("bytes"), sha1.lower()
        ) != expected[name]:
            raise PublicationError("report member bytes or digest differ from publication")
    return fresh


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("completed_report", type=argparse.FileType("r", encoding="utf-8"))
    args = parser.parse_args()
    try:
        report = json.load(args.completed_report)
        fresh = verify_publication(report)
    except (PublicationError, ValueError, OSError) as error:
        parser.exit(2, f"Publication revalidation failed: {error}\n")
    print(f"Revalidated {len(fresh['files'])} published member(s) over official HTTPS. "
          "This is not signed publisher authentication.")


if __name__ == "__main__":
    main()
