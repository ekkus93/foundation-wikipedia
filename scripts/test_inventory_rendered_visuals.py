import unittest

import inventory_rendered_visuals as vis


class InventoryTests(unittest.TestCase):
    def test_collects_sorted_deduplicated_visuals_and_separate_av_preview(self):
        page = (
            '<link rel="alternate stylesheet" href="//cdn.example/wiki.css">'
            '<style>.x{background:url(example.png)}</style>'
            '<picture><source srcset="//cdn.example/p.webp 1x, '
            'https://cdn.example/p2.webp 2x">'
            '<img src="/i.png" srcset="/i2.png 2x, /i2.png 2x"></picture>'
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

    def test_rejects_excessive_and_unsafe_references(self):
        with self.assertRaisesRegex(vis.InventoryError, "budget"):
            vis.inventory('<img src="one"><img src="two">', max_references=1)
        with self.assertRaisesRegex(vis.InventoryError, "invalid visual reference budget"):
            vis.inventory("", max_references=0)
        for value in ("https://user:password@host.invalid/a", "http://[bad-ip/a",
                      "https://example.test/" + "x" * 4200):
            with self.subTest(value=value[:24]):
                with self.assertRaises(vis.InventoryError):
                    vis.inventory('<img src="' + value + '">')

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
