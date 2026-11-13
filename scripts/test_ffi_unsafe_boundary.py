"""MOB-001 guard: unsafe Rust is allowed only in UniFFI-generated scaffolding.

Owner decision (2026-11-13): wiki-ffi alone relaxes the workspace
`unsafe_code = "forbid"` to "deny" so UniFFI's generated code (which carries
its own scoped allowance) compiles. No hand-written `unsafe`, and no manual
`allow(unsafe_code)`, may appear in wiki-ffi; every other crate keeps forbid.
"""
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FFI = ROOT / "crates/wiki-ffi"
UNSAFE = re.compile(r"\bunsafe\b")
ALLOW = re.compile(r"allow\s*\(\s*unsafe_code\s*\)")


def strip_comments(source):
    """Remove // and /* */ comments so documentation may mention unsafe."""
    source = re.sub(r"/\*.*?\*/", "", source, flags=re.S)
    return "\n".join(line.split("//", 1)[0] for line in source.splitlines())


def violations(source):
    code = strip_comments(source)
    return UNSAFE.findall(code) + ALLOW.findall(code)


class FfiUnsafeBoundaryTests(unittest.TestCase):
    def test_wiki_ffi_has_no_hand_written_unsafe(self):
        files = sorted(FFI.rglob("*.rs"))
        self.assertTrue(files)
        for path in files:
            with self.subTest(path=str(path.relative_to(ROOT))):
                self.assertEqual(violations(path.read_text(encoding="utf-8")), [])

    def test_only_wiki_ffi_relaxes_forbid_and_only_to_deny(self):
        workspace = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
        self.assertIn('unsafe_code = "forbid"', workspace)
        ffi = (FFI / "Cargo.toml").read_text(encoding="utf-8")
        self.assertIn('[lints.rust]\nunsafe_code = "deny"', ffi)
        for manifest in sorted(ROOT.glob("crates/*/Cargo.toml")) + sorted(
            ROOT.glob("tools/*/Cargo.toml")
        ):
            if manifest.parent == FFI:
                continue
            with self.subTest(crate=manifest.parent.name):
                text = manifest.read_text(encoding="utf-8")
                self.assertIn("[lints]\nworkspace = true", text)
                self.assertNotIn("unsafe_code", text)

    def test_detector_flags_real_unsafe_but_not_comments(self):
        self.assertEqual(violations("// unsafe is mentioned here\nfn f() {}"), [])
        self.assertEqual(violations("/* unsafe */ fn f() {}"), [])
        self.assertEqual(len(violations("fn f() { unsafe { g() } }")), 1)
        self.assertEqual(len(violations("#[allow(unsafe_code)]\nfn f() {}")), 1)
        self.assertEqual(len(violations("unsafe extern \"C\" fn f() {}")), 1)


if __name__ == "__main__":
    unittest.main()
