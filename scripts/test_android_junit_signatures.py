"""Guard: JUnit lifecycle and test methods in the Android app return void.

A Kotlin expression body such as `fun setUp() = runBlocking { ...; file.delete() }`
returns the block's last value, so JUnit 4 rejects the whole class
("Method ... should be void") and none of its tests run. This happened twice
on the emulator (2026-11-13). Annotated functions written with
`= runBlocking` must declare `: Unit` explicitly.
"""
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCES = [ROOT / "android/app/src/androidTest", ROOT / "android/app/src/test"]
ANNOTATED = re.compile(
    r"@(?:Test|Before|After|BeforeClass|AfterClass)\b[^\n]*\n(?:\s*@[^\n]*\n)*"
    r"\s*fun\s+(\w+)\s*\(([^)]*)\)\s*(:\s*[\w.<>?]+\s*)?=\s*runBlocking\b"
)


def offending(source):
    """Names of annotated runBlocking functions without an explicit Unit type."""
    return [
        match.group(1)
        for match in ANNOTATED.finditer(source)
        if (match.group(3) or "").replace(":", "").strip() != "Unit"
    ]


class AndroidJunitSignatureTests(unittest.TestCase):
    def test_annotated_run_blocking_functions_declare_unit(self):
        files = [path for root in SOURCES for path in sorted(root.rglob("*.kt"))]
        self.assertTrue(files)
        for path in files:
            with self.subTest(path=str(path.relative_to(ROOT))):
                self.assertEqual(offending(path.read_text(encoding="utf-8")), [])

    def test_detector(self):
        self.assertEqual(offending("@Before\nfun setUp() = runBlocking {\n}"), ["setUp"])
        self.assertEqual(
            offending("@Test\n@Ignore(\"x\")\nfun a() = runBlocking { }"), ["a"]
        )
        self.assertEqual(offending("@Test\nfun a(): Boolean = runBlocking { true }"), ["a"])
        self.assertEqual(offending("@Test\nfun a(): Unit = runBlocking { }"), [])
        self.assertEqual(offending("fun helper() = runBlocking { 1 }"), [])


if __name__ == "__main__":
    unittest.main()
