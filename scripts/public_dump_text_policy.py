"""Shared diagnostic public-dump XML and label validation policy.

Keep in parity with wiki-source public_dump_page_meta::valid_text and
public_dump_revision_jsonl::valid_xml_text. This is input validation only;
it does not establish Wikimedia publisher authenticity or completeness.
"""

import unicodedata

MAX_LABEL_BYTES = 4096
_FORBIDDEN_FORMAT = frozenset(
    ("\u061c", "\u200b", "\u200e", "\u200f", "\u2060", "\ufeff")
).union(chr(code) for code in range(0x202A, 0x202F)).union(
    chr(code) for code in range(0x2066, 0x2070)
)


def valid_xml_text(value):
    """Match the portable Rust diagnostic reader's XML 1.0 character set."""
    return isinstance(value, str) and all(
        ch in "\t\n\r"
        or "\u0020" <= ch <= "\ud7ff"
        or "\ue000" <= ch <= "\ufffd"
        or "\U00010000" <= ch <= "\U0010ffff"
        for ch in value
    )


def valid_dump_label(value):
    """Reject spoofed page titles/redirects without damaging real scripts.

    ZWNJ U+200C and ZWJ U+200D are deliberately preserved for Persian and
    Indic orthography, matching the Rust semantic XML metadata collector.
    """
    if (not isinstance(value, str) or not value.strip()
            or value != value.strip()
            or len(value.encode("utf-8", errors="surrogatepass")) > MAX_LABEL_BYTES
            or not valid_xml_text(value)):
        return False
    for ch in value:
        code = ord(ch)
        if (unicodedata.category(ch) == "Cc"
                or 0xFDD0 <= code <= 0xFDEF
                or code & 0xFFFF in (0xFFFE, 0xFFFF)
                or ch in _FORBIDDEN_FORMAT):
            return False
    return True
