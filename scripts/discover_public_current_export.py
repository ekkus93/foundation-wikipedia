#!/usr/bin/env python3
"""Fetch the completed Wikimedia current-content SHA256SUMS marker over HTTPS.

This does not download, render, or authenticate the referenced XML shards.
"""
import argparse
from datetime import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import tempfile
from urllib.error import HTTPError, URLError
from urllib.request import HTTPRedirectHandler, Request, build_opener

from extract_public_dump_export import DumpExtractionError, MAX_INVENTORY_BYTES, _inventory


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, msg, headers, newurl):
        raise DumpExtractionError("official source redirected unexpectedly")


def official_url(project, date):
    if not re.fullmatch(r"[a-z0-9_]{1,64}", project):
        raise DumpExtractionError("invalid wiki project")
    if not re.fullmatch(r"[0-9]{4}-[0-9]{2}-01", date):
        raise DumpExtractionError("invalid monthly export date")
    try:
        datetime.strptime(date, "%Y-%m-%d")
    except ValueError:
        raise DumpExtractionError("invalid monthly export date") from None
    return (f"https://dumps.wikimedia.org/other/mediawiki_content_current/"
            f"{project}/{date}/xml/bzip2/SHA256SUMS")


def discover_completed_export(project, date, output, *, opener=None):
    """Atomically publish a parsed official inventory, not verified XML bytes."""
    url = official_url(project, date)
    output = Path(output)
    if (output.exists() or output.is_symlink()
            or output.parent.is_symlink() or not output.parent.is_dir()):
        raise DumpExtractionError("unsafe or existing inventory destination")
    opener = opener if opener is not None else build_opener(NoRedirect())
    request = Request(url, headers={"User-Agent": "FoundationWikipedia/0.1"})
    try:
        with opener.open(request, timeout=30) as response:
            if response.status != 200 or response.geturl() != url:
                raise DumpExtractionError("unexpected official response")
            payload = response.read(MAX_INVENTORY_BYTES + 1)
    except HTTPError as error:
        if error.code == 404:
            raise DumpExtractionError("official monthly export is not completed") from error
        raise DumpExtractionError("official inventory HTTP failure") from error
    except (URLError, TimeoutError) as error:
        raise DumpExtractionError("official inventory HTTPS failure") from error
    if not 0 < len(payload) <= MAX_INVENTORY_BYTES:
        raise DumpExtractionError("empty or oversized inventory")
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="wb", dir=output.parent, prefix=".wikimedia-inventory.",
            delete=False,
        ) as writer:
            temporary = Path(writer.name)
            writer.write(payload)
            writer.flush()
            os.fsync(writer.fileno())
        members = _inventory(temporary, project, date)
        try:
            os.link(temporary, output)
        except FileExistsError:
            raise DumpExtractionError("inventory already exists") from None
        directory = os.open(output.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
        return {"url": url, "sha256": hashlib.sha256(payload).hexdigest(),
                "members": len(members), "shards_verified": False}
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("project")
    parser.add_argument("month")
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    print(json.dumps(discover_completed_export(args.project, args.month, args.output),
                     sort_keys=True))


if __name__ == "__main__":
    main()
