import hashlib
import json
import unittest

import inventory_rendered_visuals as vis


class InventoryTests(unittest.TestCase):
    def test_collects_sorted_deduplicated_visuals_and_separate_av_preview(self):
        page = (
            '<link rel="alternate stylesheet" href="//cdn.example/wiki.css">'
            '<style>.x{background:url(example.png)}</style>'
            '<picture><source srcset="//cdn.example/p.webp 1x, '
            'https://cdn.example/p2.webp 2x">'
            '<img src="/i.png" srcset="/i2.png 1x, /i2.png 2x"></picture>'
            '<svg><image href="//cdn.example/s.svg"/>'
            '<use href="#local"/><use xlink:href="/symbols.svg#icon"/></svg>'
            '<video poster="/thumbnail.jpg" src="//cdn.example/movie.mp4"></video>'
            '<audio src="//cdn.example/sound.mp3"></audio>'
        )
        result = vis.inventory(page)
        self.assertEqual(8, result["count"])
        self.assertEqual(1, result["inline_styles"])
        self.assertFalse(result["required_media_verified"])
        self.assertFalse(result["css_and_font_dependencies_verified"])
        self.assertEqual(result["candidates"],
                         sorted(result["candidates"], key=lambda c: (c["kind"], c["url"])))
        entries = {(x["kind"], x["url"]) for x in result["candidates"]}
        self.assertIn(("av_preview", "/thumbnail.jpg"), entries)
        self.assertIn(("svg_image", "https://cdn.example/s.svg"), entries)
        self.assertNotIn(("av_preview", "//cdn.example/movie.mp4"), entries)
        self.assertNotIn(("svg_use", "#local"), entries)

    def test_inventory_digest_is_order_independent_but_revision_content_sensitive(self):
        first = vis.inventory('<img src="a.png"><img src="b.png"><img src="a.png">')
        second = vis.inventory('<img src="b.png"><img src="a.png">')
        different = vis.inventory('<img src="b.png"><img src="c.png">')
        self.assertEqual(first["candidates"], second["candidates"])
        self.assertEqual(first["inventory_sha256"], second["inventory_sha256"])
        self.assertNotEqual(first["inventory_sha256"], different["inventory_sha256"])
        self.assertEqual(64, len(first["inventory_sha256"]))
        canonical = json.dumps(first["candidates"], ensure_ascii=True,
                               sort_keys=True, separators=(",", ":")).encode("utf-8")
        expected = hashlib.sha256(
            b"foundation-wikipedia/visual-url-candidates/v1\0" + canonical
        ).hexdigest()
        self.assertEqual(expected, first["inventory_sha256"])

    def test_rejects_excessive_and_unsafe_references(self):
        with self.assertRaisesRegex(vis.InventoryError, "budget"):
            vis.inventory('<img src="one"><img src="two">', max_references=1)
        with self.assertRaisesRegex(vis.InventoryError, "invalid visual reference budget"):
            vis.inventory("", max_references=0)
        for value in ("https://user:password@host.invalid/a", "http://[bad-ip/a",
                      "https://example.test/" + "x" * 4200,
                      "https://example.test:invalid/a",
                      "https://example.test:65536/a",
                      "https://example.test:0/a",
                      "https://:443/a"):
            with self.subTest(value=value[:24]):
                with self.assertRaises(vis.InventoryError):
                    vis.inventory('<img src="' + value + '">')

    def test_repeated_and_unsupported_references_consume_budget(self):
        # A candidate-set-only cap permits unbounded duplicate references.
        # Missing sources, unsupported schemes and same-document SVG uses
        # also require a bounded amount of parser work.
        samples = (
            '<img src="same.png"><img src="same.png">',
            '<img><img>',
            '<img src="data:image/png;base64,a"><img src="data:image/png;base64,b">',
            '<svg><use href="#one"/><use href="#one"/></svg>',
            '<picture><source srcset="a.png 1x, a.png 2x"></picture>',
        )
        for markup in samples:
            with self.subTest(markup=markup):
                with self.assertRaisesRegex(vis.InventoryError, "reference count"):
                    vis.inventory(markup, max_references=1)

    def test_observed_reference_count_is_separate_from_unique_candidates(self):
        result = vis.inventory('<img src="same.png"><img src="same.png">'
                               '<img src="data:image/png;base64,a"><img>')
        self.assertEqual(1, result["count"])
        self.assertEqual(4, result["observed_references"])
        self.assertEqual(1, result["unsupported_schemes"])
        self.assertEqual(1, result["missing_sources"])

    def test_srcset_data_url_cannot_create_phantom_relative_candidates(self):
        # A comma in a data URL is part of its payload, not necessarily a
        # separator between independent installable image variants.
        with self.assertRaisesRegex(vis.InventoryError, "data URL srcset"):
            vis.inventory('<img srcset="data:image/png;base64,AAAA 1x, '
                          '//upload.wikimedia.org/b.png 2x">')

    def test_srcset_empty_candidates_fail_closed(self):
        for srcset in ("a.png 1x,, b.png 2x", "a.png 1x, ", ",a.png 1x"):
            with self.subTest(srcset=srcset):
                with self.assertRaisesRegex(vis.InventoryError, "empty responsive image"):
                    vis.inventory('<img srcset="' + srcset + '">')

    def test_malformed_srcset_descriptors_fail_closed(self):
        invalid = ("a.png 1x 2x", "a.png 0w", "a.png 0x",
                   "a.png 2h", "a.png abc", "a.png -1x")
        for value in invalid:
            with self.subTest(value=value):
                with self.assertRaisesRegex(vis.InventoryError, "descriptor"):
                    vis.inventory('<img srcset="' + value + '">')
        self.assertEqual(2, vis.inventory(
            '<img srcset="a.png 1.5x, b.png 2x">')["count"])

    def test_duplicate_and_mixed_srcset_descriptors_fail_closed(self):
        bad = ("a.png 1x, b.png 1.0x", "a.png, b.png 1x",
               "a.png 320w, b.png 320w", "a.png 320w, b.png 2x")
        for srcset in bad:
            with self.subTest(srcset=srcset):
                with self.assertRaises(vis.InventoryError):
                    vis.inventory('<img srcset="' + srcset + '">')
        self.assertEqual(2, vis.inventory(
            '<img srcset="a.png 320w, b.png 640w">')["count"])

    def test_inline_css_dependency_surfaces_are_counted_and_bounded(self):
        page = '<style>.x{background:url(a.png)}</style><div style="background:url(b.png)"></div>'
        result = vis.inventory(page)
        self.assertEqual(1, result["inline_styles"])
        self.assertEqual(1, result["inline_style_attributes"])
        self.assertFalse(result["css_and_font_dependencies_verified"])
        with self.assertRaisesRegex(vis.InventoryError, "inline style attribute"):
            vis.inventory('<div style="a:1"></div><span style="b:2"></span>', max_references=1)
        with self.assertRaisesRegex(vis.InventoryError, "inline style element"):
            vis.inventory("<style></style><style></style>", max_references=1)

    def test_reports_unsupported_urls_without_exporting_them(self):
        result = vis.inventory(
            '<img src="data:image/png;base64,a"><img src="javascript:alert(1)">'
            '<img><img src="https://upload.wikimedia.org/static/a.png">')
        self.assertGreaterEqual(result["unsupported_schemes"], 2)
        self.assertEqual(1, result["missing_sources"])
        self.assertEqual(1, result["count"])
        self.assertEqual("https://upload.wikimedia.org/static/a.png",
                         result["candidates"][0]["url"])


if __name__ == "__main__":
    unittest.main()
