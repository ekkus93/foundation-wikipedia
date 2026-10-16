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
from urllib.parse import urljoin, urlsplit

MAX_URL_CHARS = 4096
MAX_REFERENCES = 10000
MAX_INLINE_CSS_CHARS = 2 * 1024 * 1024

# This is a diagnostic-only subset of CSS URL tokens, not a CSS parser.
# Comments and quoted CSS strings are skipped before matching url(...).
_CSS_TOKEN_RE = re.compile(
    r"""(?s)/\*.*?\*/|(?<![\w-])@import\s+(?:url\s*\(\s*(?:"([^"\\]*)"|'([^'\\]*)'|([^\s()"'\\]+))\s*\)|"([^"\\]*)"|'([^'\\]*)')|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|(?<![\w-])url\s*\(\s*(?:"([^"\\]*)"|'([^'\\]*)'|([^\s()"'\\]+))\s*\)""",
    re.IGNORECASE,
)


class InventoryError(ValueError):
    pass


class _Collector(HTMLParser):
    def __init__(self, limit, document_url=None):
        super().__init__(convert_charrefs=True)
        self.limit = limit
        # When set, every candidate is resolved to an absolute URL against the
        # HTML <base href> (if any) or this document URL; otherwise raw strings.
        self.document_url = document_url
        self.base_url = None
        self.urls = set()
        self.observed_references = 0
        self.picture_depth = 0
        self.svg_depth = 0
        self.math_depth = 0
        self.unresolved_dynamic_surfaces = 0
        self.inline_styles = 0
        self.inline_style_attributes = 0
        self.inline_css_urls = 0
        self.inline_css_imports = 0
        self.inline_css_chars = 0
        self.style_fragments = []
        self.unsupported_schemes = 0
        self.missing_sources = 0
        self.html_base_href_count = 0

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
                or "\\" in source
                or any(ch.isspace() or ord(ch) < 32 or ord(ch) == 127
                       for ch in source)):
            raise InventoryError("unsafe or oversized visual candidate URL")
        if source.startswith("#"):
            return  # Same-document SVG fragment; no separate transport URL.
        if self.document_url is not None:
            source = urljoin(self.base_url or self.document_url, source)
            if len(source) > MAX_URL_CHARS:
                raise InventoryError("unsafe or oversized visual candidate URL")
        elif source.startswith("//"):
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

    def _set_base(self, href):
        # Only the first <base href> in HTML content counts, and only if no
        # URL-bearing reference was already seen: browsers and preload scanners
        # disagree about earlier references, so that shape fails closed.
        if self.observed_references:
            raise InventoryError("base href after resource reference")
        if self.document_url is None:
            return
        if not href or href != href.strip() or href.startswith("#"):
            raise InventoryError("unsafe HTML base href")
        resolved = urljoin(self.document_url, href)
        _require_absolute_http(resolved, "HTML base href")
        self.base_url = resolved

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
            if any(token.group(i) is not None for i in range(1, 6)):
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
        resource_tags = {"img", "source", "link", "video", "image", "feimage",
                         "use", "object", "embed", "mglyph", "base"}

        if len(attrs) != len(dict(attrs)) and (
                tag in resource_tags or any(key == "style" for key, _ in attrs)):
            raise InventoryError("duplicate HTML resource attribute")
        attrs = dict(attrs)
        if "style" in attrs:
            self.inline_style_attributes += 1
            if self.inline_style_attributes > self.limit:
                raise InventoryError("inline style attribute count exceeds budget")
            self._scan_css(attrs["style"] or "")
        if (tag == "base" and "href" in attrs
                and not self.svg_depth and not self.math_depth):
            self.html_base_href_count += 1
            if self.html_base_href_count > 1:
                raise InventoryError("multiple HTML base href elements")
            self._set_base(attrs["href"])
        if tag == "picture":
            self.picture_depth += 1
        elif tag == "svg":
            self.svg_depth += 1
        elif tag == "math":
            self.math_depth += 1
        elif tag in ("canvas", "iframe", "foreignobject"):
            # Dynamic drawing, an embedded document, or arbitrary foreign
            # markup is not a verified offline visual resource graph.
            self.unresolved_dynamic_surfaces += 1
            if self.unresolved_dynamic_surfaces > self.limit:
                raise InventoryError("unresolved visual surface count exceeds budget")
        elif tag == "mglyph" and self.math_depth:
            self._add("math_glyph", attrs.get("src"))
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
        elif tag in ("image", "feimage", "use") and self.svg_depth:
            # HTMLParser lowercases SVG tag names. SVG filters can load
            # external images through feImage as well as ordinary <image>.
            href = attrs.get("href")
            legacy_href = attrs.get("xlink:href")
            if href and legacy_href and href != legacy_href:
                raise InventoryError("conflicting SVG resource attributes")
            kind = {"image": "svg_image", "feimage": "svg_filter_image",
                    "use": "svg_use"}[tag]
            self._add(kind, href or legacy_href)
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
        elif tag == "math":
            self.math_depth = max(0, self.math_depth - 1)


def _require_absolute_http(value, what):
    if not isinstance(value, str):
        raise InventoryError("invalid " + what)
    try:
        parsed = urlsplit(value)
    except ValueError as error:
        raise InventoryError("invalid " + what) from error
    if parsed.scheme not in ("http", "https") or not parsed.netloc:
        raise InventoryError(what + " must be absolute HTTP(S)")
    # Reuse the candidate checks: length, whitespace, authority and port.
    _Collector(1)._add("document", value)


def inventory(markup, max_references=MAX_REFERENCES, document_url=None):
    """List URL candidates; with document_url, resolve them to absolute URLs.

    Resolution is a diagnostic only: it does not fetch, authenticate or certify
    anything. Without document_url, candidates stay raw and base resolution is
    reported as pending.
    """
    if not isinstance(max_references, int) or max_references < 1:
        raise InventoryError("invalid visual reference budget")
    if document_url is not None:
        _require_absolute_http(document_url, "document URL")
    parser = _Collector(max_references, document_url)
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
    if document_url is None:
        domain = b"foundation-wikipedia/visual-url-candidates/v1\0"
    else:
        # Resolved candidates are bound to the document URL they came from.
        domain = (b"foundation-wikipedia/visual-url-candidates/v2\0"
                  + document_url.encode("utf-8") + b"\0")
    fingerprint = hashlib.sha256(domain + canonical).hexdigest()
    return {
        "count": len(parser.urls),
        "observed_references": parser.observed_references,
        "candidates": candidates,
        "inventory_sha256": fingerprint,
        "inline_styles": parser.inline_styles,
        "inline_style_attributes": parser.inline_style_attributes,
        "inline_css_urls": parser.inline_css_urls,
        "inline_css_imports": parser.inline_css_imports,
        "unresolved_dynamic_surfaces": parser.unresolved_dynamic_surfaces,
        "unsupported_schemes": parser.unsupported_schemes,
        "missing_sources": parser.missing_sources,
        "html_base_href_count": parser.html_base_href_count,
        "html_base_resolution_pending": (
            document_url is None and bool(parser.html_base_href_count)),
        "document_url": document_url,
        "html_base_url": parser.base_url,
        "css_and_font_dependencies_verified": False,
        "required_media_verified": False,
    }

def inventory_css_dependency_graph(roots, stylesheets, max_references=MAX_REFERENCES):
    """Discover URL candidates across supplied external CSS and @import chains.

    Caller provides stylesheet contents; this function performs no downloads
    and does not authenticate sources, parse all CSS syntax or certify packs.
    Missing discovered imports fail closed. All output URLs are absolute.
    """
    if not isinstance(max_references, int) or max_references < 1:
        raise InventoryError("invalid CSS graph reference budget")
    if not isinstance(stylesheets, dict) or not isinstance(roots, (list, tuple)):
        raise InventoryError("invalid CSS graph inputs")
    if not roots:
        raise InventoryError("empty stylesheet roots")

    def absolute_url(value):
        if not isinstance(value, str):
            raise InventoryError("invalid stylesheet URL")
        parsed = urlsplit(value)
        if parsed.scheme not in ("http", "https") or not parsed.netloc:
            raise InventoryError("stylesheet URL must be absolute HTTP(S)")
        # Apply exactly the same URL length, authority and port restrictions
        # used by the HTML candidate diagnostic.
        validator = _Collector(1)
        validator._add("stylesheet", value)
        return value

    queue = sorted(set(absolute_url(root) for root in roots))
    visited = set()
    observed = 0
    total_css_chars = 0
    found = set()
    while queue:
        current = queue.pop(0)
        if current in visited:
            continue
        if current not in stylesheets:
            raise InventoryError("missing supplied stylesheet import")
        css = stylesheets[current]
        if not isinstance(css, str):
            raise InventoryError("invalid supplied stylesheet content")
        total_css_chars += len(css)
        if total_css_chars > MAX_INLINE_CSS_CHARS:
            raise InventoryError("CSS graph text exceeds diagnostic budget")
        local = _Collector(max_references)

        local._scan_css(css)
        observed += local.observed_references
        if observed > max_references:
            raise InventoryError("CSS graph reference count exceeds budget")
        visited.add(current)
        if len(visited) > max_references:
            raise InventoryError("CSS graph stylesheet count exceeds budget")
        for kind, relative in sorted(local.urls):
            url = absolute_url(urljoin(current, relative))
            found.add((kind, url))
            if kind == "css_import" and url not in visited:
                queue.append(url)
    candidates = [{"kind": kind, "url": url} for kind, url in sorted(found)]
    transcript = json.dumps(candidates, ensure_ascii=True, sort_keys=True,
                            separators=(",", ":")).encode("utf-8")
    return {
        "stylesheets_inspected": len(visited),
        "observed_references": observed,
        "count": len(candidates),
        "candidates": candidates,
        "inventory_sha256": hashlib.sha256(
            b"foundation-wikipedia/css-url-graph/v1\0" + transcript
        ).hexdigest(),
        "css_and_font_dependencies_verified": False,
        "required_media_verified": False,
    }
