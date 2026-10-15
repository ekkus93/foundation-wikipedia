import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import check_renderer_fixtures as crf  # noqa: E402

GOOD_HTML = "<p>Hello, Wikipedia! RENDERED (8)</p>"


def page(title, html=GOOD_HTML, status=200, location=None, page_id=7, revision_id=11):
    payload = {"id": page_id, "latest": {"id": revision_id, "timestamp": "2026-10-15T00:00:00Z"}, "html": html}
    return {"title": title, "status": status, "location": location, "payload": payload}


def manifest(expect=None, observe=None):
    entry = {"title": "T", "expect": expect or {"contains": ["Hello, Wikipedia!", "RENDERED (8)"]}}
    if observe:
        entry["observe"] = observe
    return {"schema": 1, "pages": [{"title": "T", "file": "t.wiki"}], "render": [entry]}


class CheckPageTests(unittest.TestCase):
    def test_passes_when_expected_content_is_present(self):
        self.assertEqual([], crf.check_page(page("T"), {"contains": ["Hello, Wikipedia!"]}))

    def test_reports_missing_expected_content(self):
        failures = crf.check_page(page("T", "<p>nothing</p>"), {"contains": ["RENDERED (8)"]})
        self.assertEqual(1, len(failures))
        self.assertIn("missing expected content", failures[0])

    def test_unexpanded_template_or_lua_error_is_forbidden(self):
        for bad in ("<p>{{Greet|x}}</p>", '<p>Script error: No such module "Words".</p>'):
            failures = crf.check_page(page("T", bad), {})
            self.assertTrue(any("forbidden content" in f for f in failures), bad)

    def test_manifest_can_add_forbidden_strings(self):
        failures = crf.check_page(page("T"), {"not_contains": ["Hello"]})
        self.assertTrue(any("forbidden content" in f for f in failures))

    def test_non_200_and_empty_html_fail_closed(self):
        self.assertTrue(crf.check_page(page("T", status=404), {}))
        self.assertTrue(crf.check_page(page("T", html="  "), {}))

    def test_missing_or_invalid_identity_fails(self):
        for bad in (page("T", page_id=0), page("T", revision_id=-1)):
            self.assertTrue(any("invalid" in f for f in crf.check_page(bad, {})))
        result = page("T")
        result["payload"]["latest"]["timestamp"] = 5
        self.assertTrue(any("latest revision" in f for f in crf.check_page(result, {})))

    def test_redirect_via_location_header_or_body(self):
        expect = {"redirect_to": "Gravity_(fixture)"}
        by_header = {"title": "R", "status": 307, "location": "/rest.php/v1/page/Gravity_(fixture)/with_html", "payload": None}
        self.assertEqual([], crf.check_page(by_header, expect))
        body = '<link rel="mw:PageProp/redirect" href="./Gravity_(fixture)"/>'
        self.assertEqual([], crf.check_page(page("R", body), expect))
        self.assertTrue(crf.check_page(page("R", "<p>plain</p>"), expect))


class CheckResultsTests(unittest.TestCase):
    def test_two_identical_passes_succeed(self):
        passes = [[page("T")], [page("T")]]
        self.assertEqual([], crf.check_results(manifest(), passes))

    def test_single_pass_is_rejected(self):
        self.assertTrue(crf.check_results(manifest(), [[page("T")]]))

    def test_nonrepeatable_render_fails(self):
        passes = [[page("T")], [page("T", GOOD_HTML + "<!-- changed -->")]]
        failures = crf.check_results(manifest(), passes)
        self.assertTrue(any("not repeatable" in f for f in failures))

    def test_page_missing_from_a_pass_fails(self):
        failures = crf.check_results(manifest(), [[page("T")], []])
        self.assertTrue(any("missing from render pass 1" in f for f in failures))

    def test_evidence_records_observations_without_failing(self):
        passes = [[page("T")], [page("T")]]
        evidence = crf.build_evidence(manifest(observe=["mw:Extension/math"]), passes)
        self.assertEqual({"mw:Extension/math": False}, evidence["pages"][0]["observed"])
        self.assertEqual(7, evidence["pages"][0]["page_id"])
        self.assertEqual(64, len(evidence["pages"][0]["html_sha256"]))


class ManifestTests(unittest.TestCase):
    def write(self, data):
        handle = tempfile.NamedTemporaryFile("w", suffix=".json", delete=False, encoding="utf-8")
        handle.write(json.dumps(data))
        handle.close()
        self.addCleanup(Path(handle.name).unlink)
        return handle.name

    def test_rejects_wrong_schema_duplicates_and_unknown_render_entries(self):
        good = manifest()
        self.assertEqual(good, crf.load_manifest(self.write(good)))
        with self.assertRaises(ValueError):
            crf.load_manifest(self.write({**good, "schema": 2}))
        with self.assertRaises(ValueError):
            crf.load_manifest(self.write({**good, "pages": good["pages"] * 2}))
        with self.assertRaises(ValueError):
            crf.load_manifest(self.write({**good, "render": [{"title": "Other"}]}))

    def test_committed_harness_manifest_is_valid(self):
        root = Path(__file__).resolve().parent.parent / "tools" / "renderer-harness"
        loaded = crf.load_manifest(root / "manifest.json")
        for entry in loaded["pages"]:
            self.assertTrue((root / entry["file"]).is_file(), entry["file"])


if __name__ == "__main__":
    unittest.main()
