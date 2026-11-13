import hashlib
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import wpack_importer as wp  # noqa: E402

FIXTURE = Path(__file__).resolve().parents[1] / "fixtures/synthetic/wpack/roundtrip-v2.wpack"


def _envelope(manifest, blobs):
    transcript = wp.encode_transcript(manifest)
    out = wp.MAGIC + len(transcript).to_bytes(4, "big") + transcript
    for obj in sorted(manifest["objects"], key=lambda o: o["path"].encode()):
        out += blobs[obj["path"]]
    return out


def _small_manifest(blobs):
    return {
        "version": 2,
        "pack_id": "small",
        "project": "enwiki",
        "snapshot": "20261101",
        "origin": {"kind": "custom", "identity": "small-def"},
        "metadata": {
            "source": {"kind": "custom", "snapshot_id": "fixture", "manifest_sha256": "0" * 64},
            "definition_sha256": "1" * 64,
            "license": {"text_spdx": "CC-BY-SA-4.0", "attribution_object": None},
            "indexes": [],
            "counts": {"articles": 0, "redirects": 0, "media": 0},
        },
        "objects": [
            {"path": path, "sha256": hashlib.sha256(data).hexdigest(), "bytes": len(data)}
            for path, data in blobs.items()
        ],
    }


class RustFixtureRoundTrip(unittest.TestCase):
    """The committed fixture was written by the Rust writer."""

    def test_verifies_rust_written_envelope(self):
        manifest = wp.verify(FIXTURE)
        self.assertEqual(manifest["version"], 2)
        self.assertEqual(manifest["pack_id"], "roundtrip")
        meta = manifest["metadata"]
        self.assertEqual(meta["source"]["kind"], "public-dump")
        self.assertEqual(meta["license"]["attribution_object"], "attribution.json")
        self.assertEqual([i["role"] for i in meta["indexes"]], ["catalog", "fts5"])
        self.assertEqual(meta["counts"], {"articles": 2, "redirects": 1, "media": 1})
        self.assertIn("media/Ωmega.svg", [o["path"] for o in manifest["objects"]])

    def test_extract_then_rewrite_is_byte_identical(self):
        original = FIXTURE.read_bytes()
        with tempfile.TemporaryDirectory() as tmp:
            out = os.path.join(tmp, "objects")
            manifest = wp.extract(FIXTURE, out)
            for obj in manifest["objects"]:
                data = Path(out, *obj["path"].split("/")).read_bytes()
                self.assertEqual(hashlib.sha256(data).hexdigest(), obj["sha256"])
            self.assertEqual(wp.write(manifest, out), original)

    def test_cli_reports_untrusted_verification(self):
        result = subprocess.run(
            [sys.executable, str(Path(wp.__file__)), "verify", str(FIXTURE)],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(result.stdout)
        self.assertTrue(report["verified"])
        self.assertFalse(report["publisher_trust"])


class Rejections(unittest.TestCase):
    def _verify_bytes(self, data):
        with tempfile.NamedTemporaryFile(delete=False) as handle:
            handle.write(data)
        try:
            return wp.verify(handle.name)
        finally:
            os.unlink(handle.name)

    def test_tampered_payload_header_and_trailing_bytes(self):
        original = FIXTURE.read_bytes()
        for label, data in [
            ("payload bit", original[:-1] + bytes([original[-1] ^ 1])),
            ("truncated", original[:-1]),
            ("trailing", original + b"x"),
            ("magic", b"FWPACK02" + original[8:]),
            ("length", original[:8] + (0).to_bytes(4, "big") + original[12:]),
        ]:
            with self.subTest(label):
                with self.assertRaises(wp.WpackError):
                    self._verify_bytes(data)

    def test_v1_prefix_and_version_are_rejected(self):
        original = bytearray(FIXTURE.read_bytes())
        prefix_v = 12 + len(wp.PREFIX) - 2
        self.assertEqual(original[prefix_v], ord("2"))
        v1_prefix = bytearray(original)
        v1_prefix[prefix_v] = ord("1")
        version = 12 + len(wp.PREFIX)
        v1_version = bytearray(original)
        v1_version[version:version + 4] = (1).to_bytes(4, "big")
        for data in (v1_prefix, v1_version):
            with self.assertRaises(wp.WpackError):
                self._verify_bytes(bytes(data))

    def test_noncanonical_object_order_is_rejected(self):
        blobs = {"a.shard": b"aaa", "b.shard": b"bbb"}
        manifest = _small_manifest(blobs)
        good = _envelope(manifest, blobs)
        self.assertEqual(self._verify_bytes(good)["pack_id"], "small")
        transcript = bytearray(wp.encode_transcript(manifest))
        # Swap the two object records to a non-canonical order.
        tail = transcript.rfind(b"a.shard") - 8
        record = 8 + 7 + 8 + 64 + 8
        first, second = transcript[tail:tail + record], transcript[tail + record:tail + 2 * record]
        transcript[tail:tail + 2 * record] = second + first
        forged = wp.MAGIC + len(transcript).to_bytes(4, "big") + bytes(transcript)
        forged += blobs["b.shard"] + blobs["a.shard"]
        with self.assertRaises(wp.WpackError):
            self._verify_bytes(forged)

    def test_unsafe_paths_and_metadata_are_rejected(self):
        for path in ["../x", "a//b", "a\\b", "CON.txt", "é.svg", "a/", "x​", "A.shard"]:
            blobs = {"a.shard": b"aaa", path: b"zzz"}
            with self.subTest(path=path):
                with self.assertRaises(wp.WpackError):
                    wp._validate(_small_manifest(blobs))
        blobs = {"a.shard": b"aaa"}
        for label, edit in [
            ("uppercase digest", lambda m: m["metadata"].update(definition_sha256="A" * 64)),
            ("bad spdx", lambda m: m["metadata"]["license"].update(text_spdx="CC BY")),
            ("media without attribution", lambda m: m["metadata"]["counts"].update(media=1)),
            (
                "undeclared index",
                lambda m: m["metadata"]["indexes"].append(
                    {"path": "missing.sqlite", "role": "catalog", "schema": 1}
                ),
            ),
            ("traversal pack id", lambda m: m.update(pack_id="../x")),
            ("bad project", lambda m: m.update(project="EnWiki")),
        ]:
            manifest = _small_manifest(blobs)
            edit(manifest)
            with self.subTest(label):
                with self.assertRaises(wp.WpackError):
                    wp._validate(manifest)

    def test_failed_extract_leaves_no_directory(self):
        original = FIXTURE.read_bytes()
        tampered = original[:-1] + bytes([original[-1] ^ 1])
        with tempfile.TemporaryDirectory() as tmp:
            source = os.path.join(tmp, "bad.wpack")
            Path(source).write_bytes(tampered)
            out = os.path.join(tmp, "out")
            with self.assertRaises(wp.WpackError):
                wp.extract(source, out)
            self.assertFalse(os.path.exists(out))
            os.mkdir(out)
            with self.assertRaises(FileExistsError):
                wp.extract(FIXTURE, out)


if __name__ == "__main__":
    unittest.main()
