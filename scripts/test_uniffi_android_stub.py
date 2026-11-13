"""Regression tests for Android UniFFI Kotlin binding generation (MOB-001)."""
import os
from pathlib import Path
import stat
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "android/scripts/generate-uniffi-kotlin.sh"
BINDINGS = ROOT / "android/app/src/main/java/uniffi/wiki_ffi/wiki_ffi.kt"
GRADLE = ROOT / "android/app/build.gradle.kts"


class AndroidUniFfiStubTests(unittest.TestCase):
    def test_committed_bindings_and_gradle_task_are_wired(self):
        bindings = BINDINGS.read_text(encoding="utf-8")
        self.assertIn("package uniffi.wiki_ffi", bindings)
        self.assertIn("open class OfflineLibrary", bindings)
        gradle = GRADLE.read_text(encoding="utf-8")
        self.assertIn('tasks.register<Exec>("generateUniFfiKotlin")', gradle)
        self.assertIn("scripts/generate-uniffi-kotlin.sh", gradle)
        self.assertIn("net.java.dev.jna:jna", gradle)

    def test_script_invokes_generator_in_library_mode_without_formatting(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            fake = root / "uniffi-bindgen"
            capture = root / "args.txt"
            out = root / "generated"
            fake.write_text(
                "#!/usr/bin/env bash\n"
                "set -euo pipefail\n"
                "printf '%s\\n' \"$@\" > \"$CAPTURE\"\n"
                "out=''\n"
                "while [[ $# -gt 0 ]]; do\n"
                "  if [[ \"$1\" == '--out-dir' ]]; then out=\"$2\"; shift 2; else shift; fi\n"
                "done\n"
                "test -n \"$out\"\n"
                "mkdir -p \"$out\"\n"
                "printf '// fake generated Kotlin\\n' > \"$out/foundation_wikipedia.kt\"\n",
                encoding="utf-8",
            )
            fake.chmod(fake.stat().st_mode | stat.S_IXUSR)
            library = root / "libwiki_ffi.so"
            library.write_bytes(b"not really a library")
            env = os.environ.copy()
            env.update({
                "UNIFFI_BINDGEN": str(fake),
                "UNIFFI_LIBRARY": str(library),
                "UNIFFI_OUT_DIR": str(out),
                "CAPTURE": str(capture),
            })
            subprocess.run([str(SCRIPT)], cwd=ROOT, env=env, check=True,
                           capture_output=True, text=True)
            args = capture.read_text(encoding="utf-8").splitlines()
            self.assertEqual(
                args,
                ["generate", "--library", str(library), "--language", "kotlin",
                 "--no-format", "--out-dir", str(out)],
            )
            self.assertTrue((out / "foundation_wikipedia.kt").is_file())

    def test_missing_generator_fails_without_creating_output(self):
        with tempfile.TemporaryDirectory() as temp:
            out = Path(temp) / "generated"
            env = os.environ.copy()
            library = Path(temp) / "libwiki_ffi.so"
            library.write_bytes(b"stub")
            env.update({
                "UNIFFI_BINDGEN": str(Path(temp) / "missing-bindgen"),
                "UNIFFI_LIBRARY": str(library),
                "UNIFFI_OUT_DIR": str(out),
            })
            result = subprocess.run([str(SCRIPT)], cwd=ROOT, env=env,
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 2)
            self.assertIn("not executable", result.stderr)
            self.assertFalse(out.exists())


if __name__ == "__main__":
    unittest.main()
