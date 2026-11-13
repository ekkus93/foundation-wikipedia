"""AI-003 guard: the Android verifier component matches the Rust crate.

rustls-platform-verifier's Kotlin half (Maven `org.rustls:rustls-platform-
verifier`) must be exactly the version of the `rustls-platform-verifier-
android` shim crate in Cargo.lock, or JNI calls can fail at runtime. The
extra Maven repository must stay restricted to that group.
"""
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def locked_version(lock_text, name):
    match = re.search(
        r'\[\[package\]\]\nname = "' + re.escape(name) + r'"\nversion = "([^"]+)"', lock_text
    )
    return match.group(1) if match else None


class AndroidVerifierPinTests(unittest.TestCase):
    def test_gradle_pin_equals_cargo_lock(self):
        lock = (ROOT / "Cargo.lock").read_text(encoding="utf-8")
        version = locked_version(lock, "rustls-platform-verifier-android")
        self.assertIsNotNone(version)
        gradle = (ROOT / "android/app/build.gradle.kts").read_text(encoding="utf-8")
        pins = re.findall(r'"org\.rustls:rustls-platform-verifier:([^"]+)"', gradle)
        self.assertEqual(pins, [version])

    def test_verifier_repository_is_exclusive_to_its_group(self):
        settings = (ROOT / "android/settings.gradle.kts").read_text(encoding="utf-8")
        self.assertEqual(settings.count("rustls-platform-verifier/maven-archive"), 1)
        block = settings[settings.index("exclusiveContent") :]
        self.assertIn('includeGroup("org.rustls")', block[: block.index("rootProject")])

    def test_lock_parser(self):
        lock = '[[package]]\nname = "a"\nversion = "1.2.3"\n'
        self.assertEqual(locked_version(lock, "a"), "1.2.3")
        self.assertIsNone(locked_version(lock, "b"))


if __name__ == "__main__":
    unittest.main()
