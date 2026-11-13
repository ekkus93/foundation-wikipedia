#!/usr/bin/env python3
"""Independent `.wpack` (FWPACK01, manifest v2) importer and writer.

PACK-001 acceptance asks for a round trip through an importer that does not
share code with the Rust implementation. This module is written only from
ADR 0007 (docs/adr/0007-wpack-container-framing.md). It verifies an envelope
with bounded streaming reads, checks the canonical identity transcript and
every object digest, extracts objects into a new directory, and can write an
envelope from extracted objects plus parsed metadata. Bytes written here must
be identical to the Rust writer's for the same content.

It implements a conservative subset of the Rust path and identity policy
(traversal, separators, control and invisible characters, NFC, case-folding
duplicates, file/directory conflicts). The Rust implementation stays
authoritative for installation; this is an interoperability check, not an
installer, and it never establishes publisher trust.

Usage:
    wpack_importer.py verify <file.wpack>
    wpack_importer.py extract <file.wpack> <new-output-dir>
"""

import hashlib
import json
import os
import re
import sys
import unicodedata

MAGIC = b"FWPACK01"
PREFIX = b"foundation-wikipedia/manifest-identity/v2\x00"
FORMAT_VERSION = 2
MAX_MANIFEST_BYTES = 64 * 1024 * 1024
MAX_FIELD_BYTES = 4096
MAX_OBJECTS = 1_000_000
MAX_INDEXES = 16
MAX_OBJECT_BYTES = 1 << 40
MAX_DECLARED_BYTES = 4 << 40
CHUNK = 64 * 1024

SOURCE_KINDS = {1: "enterprise", 2: "public-dump", 3: "custom"}
INDEX_ROLES = {1: "catalog", 2: "fts5"}
ORIGINS = {1: "official", 2: "custom"}

SHA256 = re.compile(r"[0-9a-f]{64}\Z")
SPDX = re.compile(r"[A-Za-z0-9.+-]{1,64}\Z")
SAFE_ID = re.compile(r"[^/\\:\x00-\x1f\x7f]{1,256}\Z")
INVISIBLE = {"؜", "​", "‎", "‏", "⁠", "﻿"} | {
    chr(c) for c in list(range(0x2028, 0x202F)) + list(range(0x2066, 0x2070))
}
WINDOWS_RESERVED = {"CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"} | {
    f"{p}{n}" for p in ("COM", "LPT") for n in "123456789¹²³"
}


class WpackError(ValueError):
    """Any malformed, noncanonical or tampered envelope."""


def _read_exact(stream, count):
    data = stream.read(count)
    if data is None or len(data) != count:
        raise WpackError("truncated envelope")
    return data


class _Reader:
    """Cursor over the in-memory transcript."""

    def __init__(self, data):
        self.data = data
        self.pos = 0

    def take(self, count):
        if self.pos + count > len(self.data):
            raise WpackError("truncated transcript")
        out = self.data[self.pos:self.pos + count]
        self.pos += count
        return out

    def u8(self):
        return self.take(1)[0]

    def u32(self):
        return int.from_bytes(self.take(4), "big")

    def u64(self):
        return int.from_bytes(self.take(8), "big")

    def field(self):
        length = self.u64()
        if length > MAX_FIELD_BYTES:
            raise WpackError("field too long")
        try:
            return self.take(length).decode("utf-8")
        except UnicodeDecodeError as error:
            raise WpackError("field is not UTF-8") from error


def _unsafe_label(value):
    return any(
        unicodedata.category(c) == "Cc"
        or c in INVISIBLE
        or 0xFDD0 <= ord(c) <= 0xFDEF
        or (ord(c) & 0xFFFE) == 0xFFFE
        for c in value
    )


def _unsafe_component(component):
    if (
        component in ("", ".", "..")
        or component.endswith((".", " "))
        or unicodedata.normalize("NFC", component) != component
        or any(c in '<>"|?*' for c in component)
        or len(component.encode("utf-8")) > 255
    ):
        return True
    stem = component.upper().split(".")[0].rstrip(" ")
    return stem in WINDOWS_RESERVED


def _check_identity(value, label):
    if value != value.strip() or not SAFE_ID.match(value) or _unsafe_label(value):
        raise WpackError(f"unsafe {label}")
    if _unsafe_component(value):
        raise WpackError(f"unsafe {label}")


def _check_paths(paths):
    lower, upper, prefixes = set(), set(), {}
    for path in paths:
        if (
            len(path.encode("utf-8")) > 4096
            or "\\" in path
            or ":" in path
            or _unsafe_label(path)
            or path.count("/") >= 64
            or any(_unsafe_component(c) for c in path.split("/"))
        ):
            raise WpackError(f"unsafe object path {path!r}")
        if path.lower() in lower or path.upper() in upper:
            raise WpackError(f"duplicate object path {path!r}")
        lower.add(path.lower())
        upper.add(path.upper())
        parts = path.split("/")
        for i in range(1, len(parts) + 1):
            prefix = "/".join(parts[:i])
            for key in (prefix.lower(), prefix.upper()):
                if prefixes.setdefault(key, prefix) != prefix:
                    raise WpackError(f"case-aliased directory {prefix!r}")
    for names in (lower, upper):
        for name in names:
            parent = name
            while "/" in parent:
                parent = parent.rsplit("/", 1)[0]
                if parent in names:
                    raise WpackError(f"object {parent!r} is also a directory")


def parse_transcript(data):
    """Parse and validate a v2 identity transcript into a manifest dict."""
    reader = _Reader(data)
    if reader.take(len(PREFIX)) != PREFIX:
        raise WpackError("unsupported transcript prefix")
    version = reader.u32()
    if version != FORMAT_VERSION:
        raise WpackError(f"unsupported manifest version {version}")
    manifest = {
        "version": version,
        "pack_id": reader.field(),
        "project": reader.field(),
        "snapshot": reader.field(),
    }
    origin = reader.u8()
    if origin not in ORIGINS:
        raise WpackError("unknown origin tag")
    manifest["origin"] = {"kind": ORIGINS[origin], "identity": reader.field()}
    kind = reader.u8()
    if kind not in SOURCE_KINDS:
        raise WpackError("unknown source tag")
    source = {
        "kind": SOURCE_KINDS[kind],
        "snapshot_id": reader.field(),
        "manifest_sha256": reader.field(),
    }
    definition_sha256 = reader.field()
    text_spdx = reader.field()
    attribution_tag = reader.u8()
    if attribution_tag == 0:
        attribution = None
    elif attribution_tag == 1:
        attribution = reader.field()
    else:
        raise WpackError("unknown attribution tag")
    index_count = reader.u64()
    if index_count > MAX_INDEXES:
        raise WpackError("too many indexes")
    indexes = []
    for _ in range(index_count):
        path = reader.field()
        role = reader.u8()
        if role not in INDEX_ROLES:
            raise WpackError("unknown index role")
        indexes.append({"path": path, "role": INDEX_ROLES[role], "schema": reader.u32()})
    counts = {"articles": reader.u64(), "redirects": reader.u64(), "media": reader.u64()}
    object_count = reader.u64()
    if not 1 <= object_count <= MAX_OBJECTS:
        raise WpackError("object count out of range")
    objects = []
    for _ in range(object_count):
        objects.append({"path": reader.field(), "sha256": reader.field(), "bytes": reader.u64()})
    if reader.pos != len(data):
        raise WpackError("trailing transcript bytes")
    manifest["metadata"] = {
        "source": source,
        "definition_sha256": definition_sha256,
        "license": {"text_spdx": text_spdx, "attribution_object": attribution},
        "indexes": indexes,
        "counts": counts,
    }
    manifest["objects"] = objects
    _validate(manifest)
    if encode_transcript(manifest) != data:
        raise WpackError("noncanonical transcript")
    return manifest


def _validate(manifest):
    _check_identity(manifest["pack_id"], "pack_id")
    _check_identity(manifest["snapshot"], "snapshot")
    _check_identity(manifest["origin"]["identity"], "origin identity")
    if not re.fullmatch(r"[a-z][a-z0-9_]{0,253}[a-z0-9]|[a-z]", manifest["project"]):
        raise WpackError("invalid project")
    meta = manifest["metadata"]
    _check_identity(meta["source"]["snapshot_id"], "source snapshot")
    for digest in (meta["source"]["manifest_sha256"], meta["definition_sha256"]):
        if not SHA256.match(digest):
            raise WpackError("invalid metadata digest")
    if not SPDX.match(meta["license"]["text_spdx"]):
        raise WpackError("invalid SPDX licence")
    objects = manifest["objects"]
    paths = [o["path"] for o in objects]
    _check_paths(paths)
    declared = set(paths)
    attribution = meta["license"]["attribution_object"]
    if attribution is not None and attribution not in declared:
        raise WpackError("undeclared attribution object")
    if attribution is None and meta["counts"]["media"] > 0:
        raise WpackError("media declared without attribution")
    index_paths, roles = set(), set()
    for index in meta["indexes"]:
        if (
            index["schema"] == 0
            or index["path"] not in declared
            or index["path"] in index_paths
            or index["role"] in roles
            or index["path"] == attribution
        ):
            raise WpackError("invalid index entry")
        index_paths.add(index["path"])
        roles.add(index["role"])
    digest_sizes, total = {}, 0
    for obj in objects:
        if obj["bytes"] == 0 or obj["bytes"] > MAX_OBJECT_BYTES:
            raise WpackError("invalid object size")
        if not re.fullmatch(r"[0-9a-fA-F]{64}", obj["sha256"]):
            raise WpackError("invalid object digest")
        key = obj["sha256"].lower()
        if digest_sizes.setdefault(key, obj["bytes"]) != obj["bytes"]:
            raise WpackError("conflicting digest sizes")
        total += obj["bytes"]
        if total > MAX_DECLARED_BYTES:
            raise WpackError("declared bytes over budget")


def _field(value):
    raw = value.encode("utf-8")
    return len(raw).to_bytes(8, "big") + raw


def _path_key(item):
    return item["path"].encode("utf-8")


def encode_transcript(manifest):
    """Canonical v2 transcript: indexes and objects in byte-wise path order."""
    meta = manifest["metadata"]
    out = bytearray(PREFIX)
    out += manifest["version"].to_bytes(4, "big")
    for key in ("pack_id", "project", "snapshot"):
        out += _field(manifest[key])
    origin_tag = {v: k for k, v in ORIGINS.items()}[manifest["origin"]["kind"]]
    out.append(origin_tag)
    out += _field(manifest["origin"]["identity"])
    out.append({v: k for k, v in SOURCE_KINDS.items()}[meta["source"]["kind"]])
    out += _field(meta["source"]["snapshot_id"])
    out += _field(meta["source"]["manifest_sha256"])
    out += _field(meta["definition_sha256"])
    out += _field(meta["license"]["text_spdx"])
    if meta["license"]["attribution_object"] is None:
        out.append(0)
    else:
        out.append(1)
        out += _field(meta["license"]["attribution_object"])
    indexes = sorted(meta["indexes"], key=_path_key)
    out += len(indexes).to_bytes(8, "big")
    role_tag = {v: k for k, v in INDEX_ROLES.items()}
    for index in indexes:
        out += _field(index["path"])
        out.append(role_tag[index["role"]])
        out += index["schema"].to_bytes(4, "big")
    for key in ("articles", "redirects", "media"):
        out += meta["counts"][key].to_bytes(8, "big")
    objects = sorted(manifest["objects"], key=_path_key)
    out += len(objects).to_bytes(8, "big")
    for obj in objects:
        out += _field(obj["path"])
        out += _field(obj["sha256"].lower())
        out += obj["bytes"].to_bytes(8, "big")
    return bytes(out)


def _read_header(stream):
    if _read_exact(stream, 8) != MAGIC:
        raise WpackError("bad magic")
    length = int.from_bytes(_read_exact(stream, 4), "big")
    if not 1 <= length <= MAX_MANIFEST_BYTES:
        raise WpackError("transcript length out of range")
    return parse_transcript(_read_exact(stream, length))


def _stream_objects(stream, manifest, sink):
    for obj in sorted(manifest["objects"], key=_path_key):
        digest = hashlib.sha256()
        remaining = obj["bytes"]
        sink.begin(obj["path"])
        while remaining:
            chunk = _read_exact(stream, min(CHUNK, remaining))
            digest.update(chunk)
            sink.write(chunk)
            remaining -= len(chunk)
        if digest.hexdigest() != obj["sha256"].lower():
            raise WpackError(f"digest mismatch for {obj['path']!r}")
        sink.end()
    if stream.read(1):
        raise WpackError("trailing bytes after last object")


class _Discard:
    def begin(self, path):
        pass

    def write(self, chunk):
        pass

    def end(self):
        pass


class _Extract:
    """Write each object exclusively below a freshly created directory."""

    def __init__(self, root):
        self.root = root
        self.handle = None

    def begin(self, path):
        target = os.path.join(self.root, *path.split("/"))
        os.makedirs(os.path.dirname(target), mode=0o700, exist_ok=True)
        self.handle = open(target, "xb")

    def write(self, chunk):
        self.handle.write(chunk)

    def end(self):
        self.handle.flush()
        os.fsync(self.handle.fileno())
        self.handle.close()
        self.handle = None


def verify(path):
    """Verify every byte of an envelope file; return its manifest."""
    with open(path, "rb") as stream:
        manifest = _read_header(stream)
        _stream_objects(stream, manifest, _Discard())
    return manifest


def extract(path, output_dir):
    """Verify and extract into a new directory. Removes it on any failure."""
    os.mkdir(output_dir, 0o700)
    sink = _Extract(output_dir)
    try:
        with open(path, "rb") as stream:
            manifest = _read_header(stream)
            _stream_objects(stream, manifest, sink)
    except BaseException:
        if sink.handle is not None:
            sink.handle.close()
        for current, dirs, files in os.walk(output_dir, topdown=False):
            for name in files:
                os.remove(os.path.join(current, name))
            for name in dirs:
                os.rmdir(os.path.join(current, name))
        os.rmdir(output_dir)
        raise
    return manifest


def write(manifest, objects_dir):
    """Build envelope bytes from a manifest and its object files on disk."""
    _validate(manifest)
    transcript = encode_transcript(manifest)
    out = bytearray(MAGIC)
    out += len(transcript).to_bytes(4, "big")
    out += transcript
    for obj in sorted(manifest["objects"], key=_path_key):
        with open(os.path.join(objects_dir, *obj["path"].split("/")), "rb") as handle:
            data = handle.read()
        if len(data) != obj["bytes"] or hashlib.sha256(data).hexdigest() != obj["sha256"].lower():
            raise WpackError(f"object {obj['path']!r} does not match manifest")
        out += data
    return bytes(out)


def main(argv):
    if len(argv) == 3 and argv[1] == "verify":
        manifest = verify(argv[2])
    elif len(argv) == 4 and argv[1] == "extract":
        manifest = extract(argv[2], argv[3])
    else:
        print(__doc__.strip().split("Usage:")[1], file=sys.stderr)
        return 2
    print(json.dumps({"verified": True, "publisher_trust": False, **manifest}, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv))
    except (WpackError, OSError) as error:
        print(f"wpack_importer: {error}", file=sys.stderr)
        sys.exit(1)
