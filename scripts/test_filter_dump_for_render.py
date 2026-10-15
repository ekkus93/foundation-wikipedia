import io
import hashlib
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import filter_dump_for_render as f  # noqa: E402

HEADER = (
    '<mediawiki xmlns="http://www.mediawiki.org/xml/export-0.11/" version="0.11">\n'
    "  <siteinfo>\n    <dbname>fixturewiki</dbname>\n  </siteinfo>\n"
)


def page(title, ns, page_id, rev_id, text, redirect=False):
    redirect_line = '    <redirect title="X" />\n' if redirect else ""
    body = text.replace("<", "&lt;")
    return (
        "  <page>\n"
        f"    <title>{title}</title>\n    <ns>{ns}</ns>\n    <id>{page_id}</id>\n{redirect_line}"
        "    <revision>\n"
        f"      <id>{rev_id}</id>\n      <timestamp>2026-09-01T00:00:00Z</timestamp>\n"
        "      <contributor>\n        <username>U</username>\n        <id>9</id>\n      </contributor>\n"
        f'      <text bytes="{len(text.encode())}" xml:space="preserve">{body}</text>\n'
        "    </revision>\n  </page>\n"
    )


ARTICLE = "x" * 20 + " {{Infobox}} <ref>r</ref>"


def run(pages, **kwargs):
    source = HEADER + "".join(pages) + "</mediawiki>\n"
    out = io.StringIO()
    summary = f.filter_dump(source.splitlines(keepends=True), out, **kwargs)
    return summary, out.getvalue()


class FilterTests(unittest.TestCase):
    def test_keeps_dependency_namespaces_and_sample_only(self):
        summary, out = run(
            [
                page("Template:A", 10, 1, 11, "t"),
                page("Module:B", 828, 2, 12, "m"),
                page("Talk:Skipped", 1, 3, 13, "x"),
                page("Article One", 0, 4, 14, ARTICLE),
                page("Article Two", 0, 5, 15, ARTICLE),
            ],
            sample_limit=1,
            sample_min_bytes=10,
        )
        self.assertEqual(5, summary["source_pages"])
        self.assertEqual({"10": 1, "828": 1, "0": 1}, summary["kept_by_namespace"])
        self.assertEqual("Article One", summary["sample"][0]["title"])
        self.assertEqual(14, summary["sample"][0]["revision_id"])
        self.assertIn("Template:A", out)
        self.assertNotIn("Talk:Skipped", out)
        self.assertNotIn("Article Two", out)
        self.assertTrue(out.startswith("<mediawiki ") and out.endswith("</mediawiki>\n"))

    def test_sample_skips_redirects_short_pages_and_untemplated_text(self):
        summary, _ = run(
            [
                page("Redir", 0, 1, 11, ARTICLE, redirect=True),
                page("Short", 0, 2, 12, "{{a}} <ref>"),
                page("NoRef", 0, 3, 13, "y" * 50 + " {{a}}"),
                page("Good", 0, 4, 14, ARTICLE),
            ],
            sample_min_bytes=30,
        )
        self.assertEqual(["Good"], [s["title"] for s in summary["sample"]])

    def test_sample_hashes_decoded_unicode_wikitext_not_escaped_xml(self):
        source_text = ARTICLE + " München <math>x</math>"
        summary, _ = run([page("Mathematics", 0, 7, 17, source_text)], sample_min_bytes=10)
        sample = summary["sample"][0]
        self.assertEqual(hashlib.sha256(source_text.encode("utf-8")).hexdigest(),
                         sample["source_wikitext_sha256"])
        self.assertEqual(len(source_text.encode("utf-8")), sample["bytes"])

    def test_rejects_declared_wikitext_byte_mismatch_for_sample(self):
        source = page("Bad", 0, 7, 17, ARTICLE)
        source = source.replace(f'bytes="{len(ARTICLE.encode())}"', 'bytes="1"')
        with self.assertRaisesRegex(f.DumpFormatError, "byte length mismatch"):
            run([source], sample_min_bytes=1)

    def test_titles_are_unescaped_in_the_sample(self):
        summary, _ = run([page("A &amp; B", 0, 1, 11, ARTICLE)], sample_min_bytes=10)
        self.assertEqual("A & B", summary["sample"][0]["title"])

    def test_refuses_oversized_template_and_article_page_before_buffering(self):
        oversized = page("Template:Giant", 10, 1, 11, "x" * 300)
        regular = page("Good", 0, 2, 12, ARTICLE)
        # The non-sampled template must fail even if a sampled article follows.
        with self.assertRaisesRegex(f.DumpFormatError, "page XML exceeds byte cap"):
            run([oversized, regular], sample_min_bytes=10, max_page_xml_bytes=260)
        # A sampled page must be equally bounded.
        with self.assertRaisesRegex(f.DumpFormatError, "page XML exceeds byte cap"):
            run([regular], sample_min_bytes=10, max_page_xml_bytes=64)
        with self.assertRaisesRegex(f.DumpFormatError, "invalid page XML byte cap"):
            run([regular], sample_min_bytes=10, max_page_xml_bytes=0)
        # A comfortably sized limit preserves the previous page selection.
        report, _ = run([oversized, regular], sample_min_bytes=10,
                        max_page_xml_bytes=10240)
        self.assertEqual(2, report["source_pages"])

    def test_fails_closed_on_malformed_exports(self):
        good = page("Article", 0, 1, 11, ARTICLE)
        cases = {
            "multiple revisions": good.replace("    </revision>\n", "    </revision>\n    <revision>\n    </revision>\n"),
            "missing text bytes": good.replace('bytes="', 'size="'),
            "missing page id": good.replace("    <id>1</id>\n", ""),
        }
        for name, bad in cases.items():
            with self.assertRaises(f.DumpFormatError, msg=name):
                run([bad], sample_min_bytes=10)

    def test_fails_closed_on_truncation_and_stray_content(self):
        good = page("Article", 0, 1, 11, ARTICLE)
        for source in (HEADER + good, HEADER + good + "stray\n</mediawiki>\n", HEADER + good[:-12]):
            with self.assertRaises(f.DumpFormatError):
                f.filter_dump(source.splitlines(keepends=True), io.StringIO(), sample_min_bytes=10)
        with self.assertRaises(f.DumpFormatError):
            f.filter_dump(["<mediawiki>\n", "</mediawiki>\n"], io.StringIO())

    def test_filtered_xml_digest_reads_in_bounded_chunks(self):
        class BoundedReader(io.BytesIO):
            def __init__(self, content):
                super().__init__(content)
                self.sizes = []

            def read(self, size=-1):
                self.sizes.append(size)
                if size < 1 or size > f.HASH_CHUNK_BYTES:
                    raise AssertionError("unbounded or oversized digest read")
                return super().read(size)

        content = b"x" * (2 * f.HASH_CHUNK_BYTES + 41)
        stream = BoundedReader(content)
        self.assertEqual(hashlib.sha256(content).hexdigest(), f._sha256_stream(stream))
        self.assertGreaterEqual(len(stream.sizes), 4)
        self.assertTrue(all(n == f.HASH_CHUNK_BYTES for n in stream.sizes))
        self.assertEqual(hashlib.sha256(b"").hexdigest(), f._sha256_stream(BoundedReader(b"")))

    def test_requires_at_least_one_sample_article(self):
        with self.assertRaises(f.DumpFormatError):
            run([page("Template:A", 10, 1, 11, "t")])


if __name__ == "__main__":
    unittest.main()
