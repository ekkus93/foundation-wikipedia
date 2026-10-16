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
        for part in (value or "").split(","):
            tokens = part.strip().split(maxsplit=1)
            if len(tokens) > 2:
                raise InventoryError("invalid responsive image descriptor")
            if len(tokens) == 2:
                descriptor = tokens[1]
                valid = (len(descriptor) <= 32 and re.fullmatch(
                    r"(?:[1-9][0-9]*w|(?:[0-9]+(?:\\.[0-9]*)?|\\.[0-9]+)x)",
                    descriptor,
                ))
                if not valid or (descriptor.endswith("x") and float(descriptor[:-1]) <= 0):
                    raise InventoryError("invalid responsive image descriptor")
            if tokens:
                if tokens[0].lower().startswith("data:"):
                    raise InventoryError("data URL srcset requires complete parsing")
                self._add(kind, tokens[0])

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag == "picture":
            self.picture_depth += 1
        elif tag == "svg":
            self.svg_depth += 1
        elif tag == "img":
            self._add("image", attrs.get("src"))
            self._srcset("responsive_image", attrs.get("srcset"))
        elif tag == "source" and self.picture_depth:
            self._srcset("picture_variant", attrs.get("srcset"))
        elif tag == "link" and "stylesheet" in (attrs.get("rel") or "").lower().split():
            self._add("stylesheet", attrs.get("href"))
        elif tag == "video" and attrs.get("poster"):
            self._add("av_preview", attrs["poster"])
        elif tag == "image" and self.svg_depth:
            self._add("svg_image", attrs.get("href") or attrs.get("xlink:href"))
        elif tag == "use" and self.svg_depth:
            self._add("svg_use", attrs.get("href") or attrs.get("xlink:href"))
        elif tag == "style":
            self.inline_styles += 1

    def handle_endtag(self, tag):
        if tag == "picture":
            self.picture_depth = max(0, self.picture_depth - 1)
        elif tag == "svg":
            self.svg_depth = max(0, self.svg_depth - 1)


def inventory(markup, max_references=MAX_REFERENCES):
    if not isinstance(max_references, int) or max_references < 1:
        raise InventoryError("invalid visual reference budget")
    parser = _Collector(max_references)
    parser.feed(markup)
    parser.close()
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
        "unsupported_schemes": parser.unsupported_schemes,
        "missing_sources": parser.missing_sources,
        "css_and_font_dependencies_verified": False,
        "required_media_verified": False,
    }
