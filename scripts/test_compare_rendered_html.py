import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import compare_rendered_html as c  # noqa: E402

PAGE = (
    "<h2>Early life</h2><p>Text "
    '<sup typeof="mw:Extension/ref" class="mw-ref"><a href="#cite_note-1">[1]</a></sup> '
    '<a rel="mw:WikiLink" href="./Alpha">Alpha</a> '
    '<a rel="mw:WikiLink" href="./Beta%20Two">Beta</a> '
    '<a rel="mw:WikiLink" href="./Gamma#Sec">skip</a></p>'
    '<table class="wikitable"><tr><td>x</td></tr></table><h3>Later</h3>'
)


class SummaryTests(unittest.TestCase):
    def test_extracts_structure(self):
        s = c.summarize(PAGE)
        self.assertEqual([(2, "Early life"), (3, "Later")], s["headings"])
        self.assertEqual(1, s["tables"])
        self.assertEqual(1, s["refs"])
        self.assertEqual(["Alpha", "Beta Two"], s["links"])
        self.assertEqual({}, s["error_markers"])

    def test_counts_error_markers(self):
        s = c.summarize('<p>Script error: bad</p><span class="error">x</span><p>Script error</p>')
        self.assertEqual({"Script error": 2, 'class="error': 1}, s["error_markers"])


class CompareTests(unittest.TestCase):
    def test_identical_pages_score_perfectly(self):
        m = c.compare(c.summarize(PAGE), c.summarize(PAGE))
        self.assertEqual(1.0, m["heading_similarity"])
        self.assertEqual(1.0, m["link_jaccard"])
        self.assertEqual((0, 0, 0, 0), (m["table_delta"], m["ref_delta"], m["local_error_markers"], m["live_error_markers"]))

    def test_differences_are_measured(self):
        local = c.summarize(PAGE.replace("Alpha", "Delta").replace("<table", "<p").replace("</table>", "</p>"))
        m = c.compare(local, c.summarize(PAGE))
        self.assertLess(m["link_jaccard"], 1.0)
        self.assertEqual(-1, m["table_delta"])

    def test_link_mismatch_samples_are_stable_and_bounded(self):
        local = {"headings": [], "tables": 0, "refs": 0, "error_markers": {},
                 "links": [f"P{i:02}" for i in range(15)]}
        live = {"headings": [], "tables": 0, "refs": 0, "error_markers": {},
                "links": [f"Q{i:02}" for i in range(16)]}
        metrics = c.compare(local, live)
        self.assertEqual(16, metrics["missing_link_count"])
        self.assertEqual(15, metrics["unexpected_link_count"])
        self.assertEqual([f"Q{i:02}" for i in range(10)], metrics["missing_links_sample"])
        self.assertEqual([f"P{i:02}" for i in range(10)], metrics["unexpected_links_sample"])

    def test_local_error_aggregate_identifies_page_and_revision(self):
        metrics = c.compare(c.summarize("<p>Script error</p>"), c.summarize(PAGE))
        report = c.aggregate([{
            "title": "Revision test", "source_revision_id": 777,
            "local_error_details": {"Script error": 1}, "metrics": metrics,
        }])
        self.assertEqual(1, report["pages_below_half_link_overlap"])
        self.assertEqual([{"title": "Revision test", "source_revision_id": 777,
                           "markers": {"Script error": 1}}], report["local_error_page_samples"])

    def test_upstream_error_is_distinguished_from_local_regression(self):
        broken = c.summarize('<p>Lua error in Module:Convert</p>')
        clean = c.summarize('<p>ok</p>')
        upstream = c.compare(broken, broken)
        self.assertEqual(1, upstream["live_error_markers"])
        self.assertEqual({}, upstream["error_marker_delta"])
        regression = c.compare(broken, clean)
        self.assertEqual({"Lua error": 1}, regression["error_marker_delta"])
        records = [{"metrics": upstream}, {"metrics": regression}]
        summary = c.aggregate(records)
        self.assertEqual(2, summary["pages_with_local_errors"])
        self.assertEqual(1, summary["pages_with_live_errors"])
        self.assertEqual(1, summary["pages_with_local_only_errors"])
        self.assertEqual(1, summary["pages_with_error_marker_drift"])

    def test_redlink_action_does_not_change_target_identity(self):
        local = c.summarize('<a rel="mw:WikiLink" href="./Alpha?action=edit&amp;redlink=1">A</a>')
        live = c.summarize('<a rel="mw:WikiLink" href="./Alpha">A</a>')
        self.assertEqual(["Alpha"], local["links"])
        self.assertEqual(["Alpha"], local["redlinks"])
        metric = c.compare(local, live)
        self.assertEqual(1.0, metric["link_jaccard"])
        self.assertEqual(1, metric["redlink_status_drift_count"])
        self.assertEqual(0, metric["missing_link_count"])
        self.assertEqual(0, metric["unexpected_link_count"])

    def test_arbitrary_queries_and_section_fragments_do_not_alias_targets(self):
        content = ('<a rel="mw:WikiLink" href="./Alpha?diff=42">A</a>'
                   '<a rel="mw:WikiLink" href="./Bravo#Sec">B</a>'
                   '<a rel="mw:WikiLink" href="./Charlie">C</a>')
        self.assertEqual(["Charlie"], c.summarize(content)["links"])

    def test_renderer_error_excerpt_is_bounded_and_readable(self):
        markup = ('<p>before</p><span class="error">Lua error: missing module X' + 'x' * 500 + '</span>')
        excerpt = c._error_excerpt(markup)
        self.assertIn("Lua error: missing module X", excerpt)
        self.assertLessEqual(len(excerpt), 180)

    def test_empty_pages_do_not_divide_by_zero(self):
        m = c.compare(c.summarize(""), c.summarize(""))
        self.assertEqual(1.0, m["link_jaccard"])

    def test_aggregate_counts_pages_with_problems(self):
        good = {"metrics": c.compare(c.summarize(PAGE), c.summarize(PAGE))}
        bad = {"metrics": c.compare(c.summarize("<p>Script error</p>"), c.summarize(PAGE))}
        agg = c.aggregate([good, bad])
        self.assertEqual(2, agg["pages"])
        self.assertEqual(1, agg["pages_with_local_errors"])
        self.assertEqual(1, agg["pages_with_ref_mismatch"])
        self.assertEqual({"pages": 0}, c.aggregate([]))


if __name__ == "__main__":
    unittest.main()
