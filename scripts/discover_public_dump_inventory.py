#!/usr/bin/env python3
"""Discover a completed official Wikimedia current-content checksum publication.

SRC-003 acquisition increment: this stages an authenticated-origin SHA256SUMS
inventory only. It does not download shards, render wikitext or activate packs.
"""
import argparse
from datetime import datetime
import hashlib
from html.parser import HTMLParser
import os
from pathlib import Path
import re
import tempfile
from urllib.error import HTTPError
from urllib.parse import urlsplit
from urllib.request import HTTPRedirectHandler, Request, build_opener


BASE = "https://dumps.wikimedia.org/other/mediawiki_content_current/"
MAX_INDEX_BYTES = 1024 * 1024
MAX_CHECKSUM_BYTES = 32 * 1024 * 1024


class PublicationError(ValueError):
    """An official current-content publication cannot be established."""


def _official_url(url):
    parsed = urlsplit(url)
    if (parsed.scheme != "https" or parsed.netloc != "dumps.wikimedia.org"
            or not parsed.path.startswith("/other/mediawiki_content_current/")
            or parsed.query or parsed.fragment):
        raise PublicationError("unexpected checksum publication origin")
    return url


class _OfficialRedirect(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        _official_url(newurl)
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def fetch_official(url, limit):
    _official_url(url)
    request = Request(url, headers={"User-Agent": "foundation-wikipedia-src003/0.1"})
    with build_opener(_OfficialRedirect()).open(request, timeout=30) as response:
        _official_url(response.geturl())
        data = response.read(limit + 1)
        if len(data) > limit:
            raise PublicationError("official publication response exceeds budget")
        return data


class _DirectoryLinks(HTMLParser):
    def __init__(self):
        super().__init__()
        self.links = []

    def handle_starttag(self, tag, attrs):
        if tag.lower() == "a":
            self.links.extend(value for key, value in attrs
                              if key.lower() == "href" and value is not None)


def _release_date(link):
    if not re.fullmatch(r"[0-9]{4}-[0-9]{2}-01/", link):
        return None
    try:
        datetime.strptime(link[:-1], "%Y-%m-%d")
    except ValueError:
        return None
    return link[:-1]


def _windows_device_name(part):
    """Mirror the Rust checksum parser's reserved-device path guard."""
    base = part.split(".", 1)[0].upper()
    return (base in ("CON", "PRN", "AUX", "NUL")
            or (len(base) == 4 and base[:3] in ("COM", "LPT")
                and base[3] in "123456789"))


def _check_inventory(data, project, date):
    try:
        lines = data.decode("utf-8").splitlines()
    except UnicodeDecodeError:
        raise PublicationError("invalid checksum inventory encoding") from None
    if not lines or len(lines) > 100_000:
        raise PublicationError("empty or oversized checksum inventory")
    names = set()
    partitions = []
    prefix = f"{project}-{date}-p"
    for line in lines:
        match = re.fullmatch(r"([0-9a-fA-F]{64}) [ *]([a-zA-Z0-9_./-]+)", line)
        if not match:
            raise PublicationError("invalid checksum inventory member")
        path = match.group(2)
        parts = path.split("/")
        if (any(not part or len(part) > 255 or part in (".", "..")
                or part.endswith(".") or _windows_device_name(part)
                for part in parts) or len(parts) > 64 or len(path) > 4096):
            raise PublicationError("unsafe checksum inventory path")
        name = parts[-1]
        suffix = name.removeprefix(prefix).removesuffix(".xml.bz2")
        page_range = re.fullmatch(r"([0-9]+)p([0-9]+)", suffix)
        if not name.startswith(prefix) or not name.endswith(".xml.bz2") or page_range is None:
            raise PublicationError("unexpected current-content member")
        first, last = map(int, page_range.groups())
        if not 0 < first <= last <= 2**64 - 1:
            raise PublicationError("unexpected current-content member")
        if path.lower() in names:
            raise PublicationError("duplicate checksum inventory member")
        names.add(path.lower())
        partitions.append((first, last))
    partitions.sort()
    if any(previous[1] >= current[0]
           for previous, current in zip(partitions, partitions[1:])):
        raise PublicationError("overlapping current-content page partitions")
    return len(names)


def discover_completed_export(project, fetch=fetch_official):
    """Find newest published checksum inventory, skipping incomplete dates."""
    if not re.fullmatch(r"[a-z0-9_]+", project):
        raise PublicationError("invalid wiki project")
    index_url = BASE + project + "/"
    directory = _DirectoryLinks()
    try:
        directory.feed(fetch(index_url, MAX_INDEX_BYTES).decode("utf-8"))
    except UnicodeDecodeError:
        raise PublicationError("invalid official directory encoding") from None
    dates = sorted({date for link in directory.links
                    if (date := _release_date(link)) is not None}, reverse=True)
    if not dates:
        raise PublicationError("no dated official exports found")
    for date in dates:
        url = index_url + date + "/xml/bzip2/SHA256SUMS"
        try:
            inventory = fetch(url, MAX_CHECKSUM_BYTES)
        except HTTPError as error:
            if error.code == 404:
                continue
            raise
        members = _check_inventory(inventory, project, date)
        return {"project": project, "date": date, "checksum_url": url,
                "sha256": hashlib.sha256(inventory).hexdigest(),
                "member_count": members}, inventory
    raise PublicationError("no completed official export checksum publication")


def save_inventory(output, data):
    """Publish complete inventory without overwriting an existing file."""
    output = Path(output)
    if not output.parent.is_dir() or output.parent.is_symlink():
        raise PublicationError("unsafe output directory")
    if output.exists() or output.is_symlink():
        raise PublicationError("inventory output already exists")
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="wb", dir=output.parent,
                                         prefix=".dump-inventory.", delete=False) as writer:
            temporary = Path(writer.name)
            writer.write(data)
            writer.flush()
            os.fsync(writer.fileno())
        try:
            os.link(temporary, output)
        except FileExistsError:
            raise PublicationError("inventory output already exists") from None
        descriptor = os.open(output.parent, os.O_RDONLY)
        try:
            os.fsync(descriptor)
        finally:
            os.close(descriptor)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def open_official_stream(url):
    """Open a verified-origin response for bounded streaming by the caller."""
    _official_url(url)
    request = Request(url, headers={"User-Agent": "foundation-wikipedia-src003/0.1"})
    response = build_opener(_OfficialRedirect()).open(request, timeout=30)
    try:
        _official_url(response.geturl())
    except BaseException:
        response.close()
        raise
    return response


def download_verified_shard(project, member, output, *,
                            fetch=fetch_official, stream=open_official_stream,
                            max_compressed_bytes=1024 * 1024 * 1024):
    """Discover publication and stage one SHA-256 verified compressed member.

    No partial download is published. This is a full-fetch transport; resume
    support and integration with the Rust staged verifier remain future work.
    """
    if not 0 < max_compressed_bytes <= 2**34:
        raise PublicationError("invalid compressed download budget")
    metadata, inventory = discover_completed_export(project, fetch)
    _check_inventory(inventory, project, metadata["date"])
    digest = None
    for line in inventory.decode("utf-8").splitlines():
        if line[66:] == member:
            digest = line[:64].lower()
            break
    if digest is None:
        raise PublicationError("member absent from completed checksum inventory")
    output = Path(output)
    if not output.parent.is_dir() or output.parent.is_symlink():
        raise PublicationError("unsafe output directory")
    if output.exists() or output.is_symlink():
        raise PublicationError("shard output already exists")
    url = metadata["checksum_url"].removesuffix("SHA256SUMS") + member
    _official_url(url)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="wb", dir=output.parent,
                                         prefix=".dump-shard.", delete=False) as writer:
            temporary = Path(writer.name)
            sha = hashlib.sha256()
            size = 0
            with stream(url) as response:
                while True:
                    chunk = response.read(1024 * 1024)
                    if not chunk:
                        break
                    size += len(chunk)
                    if size > max_compressed_bytes:
                        raise PublicationError("compressed download exceeds budget")
                    sha.update(chunk)
                    writer.write(chunk)
            if size == 0 or sha.hexdigest() != digest:
                raise PublicationError("compressed download SHA-256 mismatch")
            writer.flush()
            os.fsync(writer.fileno())
        try:
            os.link(temporary, output)
        except FileExistsError:
            raise PublicationError("shard output already exists") from None
        descriptor = os.open(output.parent, os.O_RDONLY)
        try:
            os.fsync(descriptor)
        finally:
            os.close(descriptor)
        return {"publication": metadata, "member": member,
                "sha256": digest, "bytes": size, "url": url}
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("project")
    parser.add_argument("output_sha256sums", type=Path)
    args = parser.parse_args()
    metadata, inventory = discover_completed_export(args.project)
    save_inventory(args.output_sha256sums, inventory)
    print(f"Official checksum inventory staged: {metadata}")


if __name__ == "__main__":
    main()
