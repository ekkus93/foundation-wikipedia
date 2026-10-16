#!/usr/bin/env python3
"""Bounded read-only visual-URL candidates from rendered HTML.

No downloads, redirects, browser execution, publisher trust, or licensing
claims are made by this diagnostic extractor. A complete CSS and HTML resource
dependency graph will require a separate verified renderer/media pipeline.
"""
import hashlib
import json
import re
from html.parser import HTMLParser
from urllib.parse import urlsplit

MAX_URL_CHARS = 4096
MAX_REFERENCES = 10000
MAX_INLINE_CSS_CHARS = 2 * 1024 * 1024

# This is a diagnostic-only subset of CSS URL tokens, not a CSS parser.
# Comments and quoted CSS strings are skipped before matching url(...).
_CSS_TOKEN_RE = re.compile(
    r"""(?s)/\*.*?\*/|(?<![\w-])@import\s+(?:"([^"\\]*)"|'([^'\\]*)')|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|(?<![\w-])url\s*\(\s*(?:"([^"\\]*)"|'([^'\\]*)'|([^\s()"'\\]+))\s*\)""",
    re.IGNORECASE,
)


class InventoryError(ValueError):
    pass


class _Collector(HTMLParser):
    def __init__(self, limit):
        super().__init__(convert_charrefs=True)
        self.limit = limit
        self.urls = set()
        self.observed_references = 0
        self.picture_depth = 0
        self.svg_depth = 0
        self.inline_styles = 0
        self.inline_style_attributes = 0
        self.inline_css_urls = 0
        self.inline_css_imports = 0
        self.inline_css_chars = 0
        self.style_fragments = []
        self.unsupported_schemes = 0
        self.missing_sources = 0

    def _add(self, kind, source):
        # Cap total observed references, not only distinct URL candidates:
        # repeated, missing and unsupported references must consume budget.
        self.observed_references += 1
        if self.observed_references > self.limit:
            raise InventoryError("visual reference count exceeds budget")
        if not source:
            self.missing_sources += 1
            return
        if (len(source) > MAX_URL_CHARS or source != source.strip()
                or any(ch.isspace() or ord(ch) < 32 for ch in source)):
            raise InventoryError("unsafe or oversized visual candidate URL")
        if source.startswith("#"):
            return  # Same-document SVG fragment; no separate transport URL.
        if source.startswith("//"):
            source = "https:" + source
        try:
            parsed = urlsplit(source)
        except ValueError as error:
            raise InventoryError("invalid visual candidate URL") from error
        if parsed.scheme:
            if parsed.scheme not in ("http", "https"):
                self.unsupported_schemes += 1
                return
            if not parsed.netloc or "@" in parsed.netloc or not parsed.hostname:
                raise InventoryError("unsafe visual candidate authority")
            try:
                port = parsed.port
            except ValueError as error:
                raise InventoryError("invalid visual candidate port") from error
            if port == 0:
                raise InventoryError("invalid visual candidate port")
        elif parsed.netloc:
            raise InventoryError("ambiguous visual candidate authority")
        self.urls.add((kind, source))
        if len(self.urls) > self.limit:
            raise InventoryError("visual candidate count exceeds budget")

    def _srcset(self, kind, value):
        # Deliberately a diagnostic split: data URIs containing commas require
        # a production-compliant parser before offline completeness approval.
        if value and any(not part.strip() for part in value.split(",")):
            raise InventoryError("empty responsive image candidate")
        seen_descriptors = set()
        descriptor_family = None
        for part in (value or "").split(","):
            tokens = part.strip().split(maxsplit=1)
            if len(tokens) > 2:
                raise InventoryError("invalid responsive image descriptor")
            if len(tokens) == 2:
                descriptor = tokens[1]
                valid = (len(descriptor) <= 32 and re.fullmatch(
                    r"(?:[1-9][0-9]*w|(?:[0-9]+(?:[.][0-9]*)?|[.][0-9]+)x)",
                    descriptor,
                ))
                if not valid or (descriptor.endswith("x") and float(descriptor[:-1]) <= 0):
                    raise InventoryError("invalid responsive image descriptor")
            if tokens:
                descriptor = tokens[1] if len(tokens) == 2 else "1x"
                family = descriptor[-1]
                value_number = (int(descriptor[:-1]) if family == "w"
                                else float(descriptor[:-1]))
                if descriptor_family is not None and family != descriptor_family:
                    raise InventoryError("mixed responsive image descriptors")
                if (family, value_number) in seen_descriptors:
                    raise InventoryError("duplicate responsive image descriptor")
                descriptor_family = family
                seen_descriptors.add((family, value_number))
                if tokens[0].lower().startswith("data:"):
                    raise InventoryError("data URL srcset requires complete parsing")
                self._add(kind, tokens[0])

    def _scan_css(self, css):
        self.inline_css_chars += len(css)
        if self.inline_css_chars > MAX_INLINE_CSS_CHARS:
            raise InventoryError("inline CSS text exceeds diagnostic budget")
        for token in _CSS_TOKEN_RE.finditer(css):
            source = next((s for s in token.groups() if s is not None), None)
            if source is None:
                continue  # CSS comment or quoted string, not a URL token.
            if token.group(1) is not None or token.group(2) is not None:
                self.inline_css_imports += 1
                self._add("css_import", source)
            else:
                self.inline_css_urls += 1
                self._add("css_url", source)

    def handle_data(self, data):
        if self.style_fragments:
            self.inline_css_chars += len(data)
            if self.inline_css_chars > MAX_INLINE_CSS_CHARS:
                raise InventoryError("inline CSS text exceeds diagnostic budget")
            self.style_fragments[-1].append(data)
            self.inline_css_chars -= len(data)  # Count once when scanning.

    def handle_starttag(self, tag, attrs):
        # A duplicate resource attribute is ambiguous: browsers and dict()
        # can choose different occurrences and hide a required dependency.
        resource_tags = {"img", "source", "link", "video", "image", "use",
                         "object", "embed"}
        if len(attrs) != len(dict(attrs)) and (
                tag in resource_tags or any(key == "style" for key, _ in attrs)):
            raise InventoryError("duplicate HTML resource attribute")
        attrs = dict(attrs)
        if "style" in attrs:
            self.inline_style_attributes += 1
            if self.inline_style_attributes > self.limit:
                raise InventoryError("inline style attribute count exceeds budget")
            self._scan_css(attrs["style"] or "")
        if tag == "picture":
            self.picture_depth += 1
        elif tag == "svg":
            self.svg_depth += 1
        elif tag == "img":
            self._add("image", attrs.get("src"))
            self._srcset("responsive_image", attrs.get("srcset"))
        elif tag == "source" and self.picture_depth:
            self._srcset("picture_variant", attrs.get("srcset"))
        elif tag == "link":
            relations = set((attrs.get("rel") or "").lower().split())
            if "stylesheet" in relations:
                self._add("stylesheet", attrs.get("href"))
            elif "preload" in relations:
                resource_type = (attrs.get("as") or "").lower().strip()
                if resource_type in ("font", "image", "style"):
                    self._add("preload_" + resource_type, attrs.get("href"))
                    if resource_type == "image" and "imagesrcset" in attrs:
                        self._srcset("preload_responsive_image", attrs["imagesrcset"])
        elif tag in ("object", "embed"):
            # Only explicitly image-typed embeds can be classified safely.
            media_type = (attrs.get("type") or "").split(";", 1)[0].strip().lower()
            if media_type.startswith("image/"):
                self._add("embedded_image", attrs.get(
                    "data" if tag == "object" else "src"))
        elif tag == "video" and attrs.get("poster"):
            self._add("av_preview", attrs["poster"])
        elif tag == "image" and self.svg_depth:
            self._add("svg_image", attrs.get("href") or attrs.get("xlink:href"))
        elif tag == "use" and self.svg_depth:
            self._add("svg_use", attrs.get("href") or attrs.get("xlink:href"))
        elif tag == "style":
            self.inline_styles += 1
            if self.inline_styles > self.limit:
                raise InventoryError("inline style element count exceeds budget")
            self.style_fragments.append([])

    def handle_endtag(self, tag):
        if tag == "style" and self.style_fragments:
            self._scan_css("".join(self.style_fragments.pop()))
        elif tag == "picture":
            self.picture_depth = max(0, self.picture_depth - 1)
        elif tag == "svg":
            self.svg_depth = max(0, self.svg_depth - 1)


def inventory(markup, max_references=MAX_REFERENCES):
    if not isinstance(max_references, int) or max_references < 1:
        raise InventoryError("invalid visual reference budget")
    parser = _Collector(max_references)
    parser.feed(markup)
    parser.close()
    if parser.style_fragments:
        raise InventoryError("unclosed inline style element")
    candidates = [{"kind": kind, "url": url}
                  for kind, url in sorted(parser.urls)]
    # Stable measurement identity only. Neither source authenticity nor the
    # bytes at these URLs is established by this diagnostic digest.
    canonical = json.dumps(candidates, ensure_ascii=True,
                           sort_keys=True, separators=(",", ":")).encode("utf-8")
    fingerprint = hashlib.sha256(
        b"foundation-wikipedia/visual-url-candidates/v1\0" + canonical
    ).hexdigest()
    return {
        "count": len(parser.urls),
        "observed_references": parser.observed_references,
        "candidates": candidates,
        "inventory_sha256": fingerprint,
        "inline_styles": parser.inline_styles,
        "inline_style_attributes": parser.inline_style_attributes,
        "inline_css_urls": parser.inline_css_urls,
        "inline_css_imports": parser.inline_css_imports,
        "unsupported_schemes": parser.unsupported_schemes,
        "missing_sources": parser.missing_sources,
        "css_and_font_dependencies_verified": False,
        "required_media_verified": False,
    }
