//! PACK-003: SVG drawings kept as vectors, so labels stay sharp at any zoom.
//!
//! A pack may hold an SVG only in the exact form [`sanitize_svg`] writes:
//! a fresh serialization of an allowlist of drawing elements. The builder
//! keeps an SVG only when sanitising removed nothing that is drawn, and the
//! installer re-runs the sanitiser on every packed SVG and refuses one that
//! is not already in that form ([`is_clean_svg`]), so a hostile custom pack
//! cannot smuggle markup past the builder.
//!
//! What a clean SVG cannot contain:
//!
//! * scripts, event handlers, `foreignObject`, embedded HTML, `<image>`,
//!   animation, fonts, or any element outside the allowlist;
//! * a reference to anything outside the file: every `href` and every
//!   `url(…)` must point at `#id` inside it, and CSS may not `@import`,
//!   escape characters or name another resource;
//! * a DTD, processing instructions or comments. A source's DOCTYPE is
//!   accepted only if its internal subset holds nothing but plain text
//!   entities (as Adobe Illustrator writes for namespace names); the
//!   sanitiser expands those itself, within a size bound, and drops the
//!   DOCTYPE, so the XML parser never sees a DTD.
//!
//! Editor metadata (`<metadata>`, `<title>`, `<desc>`, Inkscape and
//! Sodipodi namespaces) and whitespace between elements are dropped; they
//! are never drawn. A link (`<a>`) becomes a plain group. Anything else that
//! is not allowed makes the whole file [`SvgRejected`], and the builder uses
//! Wikimedia's raster rendering instead.
//!
//! The reader shows SVGs only as `<img>` sources, where browsers run no
//! script and load nothing, and the apps serve them with a CSP that forbids
//! every load; the allowlist is the first of those three layers.

use std::fmt::Write;

const SVG_NS: &str = "http://www.w3.org/2000/svg";
const XLINK_NS: &str = "http://www.w3.org/1999/xlink";
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

/// Largest SVG a pack may hold, after sanitising.
pub const MAX_PACK_SVG_BYTES: usize = 1024 * 1024;
/// Largest SVG source the sanitiser reads.
pub const MAX_SVG_SOURCE_BYTES: usize = 4 * 1024 * 1024;
const MAX_NODES: u32 = 200_000;
const MAX_DEPTH: usize = 128;

/// Why an SVG cannot be packed as a vector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SvgRejected {
    TooLarge,
    /// Not UTF-8 XML, has a DTD, or exceeds the node limit.
    Malformed(String),
    NotSvg,
    TooDeep,
    /// An element outside the allowlist, e.g. `script` or `image`.
    Element(String),
    /// An event handler, an external reference or unsafe CSS.
    Attribute(String),
    UnsafeCss,
    /// A processing instruction other than none, e.g. `xml-stylesheet`.
    Instruction(String),
}

/// Drawing elements kept as they are.
const ELEMENTS: [&str; 57] = [
    "svg",
    "g",
    "defs",
    "symbol",
    "use",
    "switch",
    "path",
    "rect",
    "circle",
    "ellipse",
    "line",
    "polyline",
    "polygon",
    "text",
    "tspan",
    "textPath",
    "linearGradient",
    "radialGradient",
    "stop",
    "pattern",
    "clipPath",
    "mask",
    "marker",
    "style",
    "filter",
    "feBlend",
    "feColorMatrix",
    "feComponentTransfer",
    "feFuncR",
    "feFuncG",
    "feFuncB",
    "feFuncA",
    "feComposite",
    "feConvolveMatrix",
    "feDiffuseLighting",
    "feDisplacementMap",
    "feDistantLight",
    "feDropShadow",
    "feFlood",
    "feGaussianBlur",
    "feMerge",
    "feMergeNode",
    "feMorphology",
    "feOffset",
    "fePointLight",
    "feSpecularLighting",
    "feSpotLight",
    "feTile",
    "feTurbulence",
    // Kept, but only as a group: the link itself is removed.
    "a",
    // Never drawn: dropped with their content.
    "title",
    "desc",
    "metadata",
    // Legacy/no-op containers some editors write; dropped with content.
    "sodipodi",
    "namedview",
    "view",
    "flowRoot",
];

/// Elements removed with their content because they are never drawn.
/// Inkscape's SVG 1.2 flowed text (`flowRoot`) is drawn neither by
/// browsers nor by Wikimedia's renderer.
fn never_drawn(name: &str) -> bool {
    matches!(
        name,
        "title" | "desc" | "metadata" | "sodipodi" | "namedview" | "view" | "flowRoot"
    )
}

/// Elements whose text is drawn (or is CSS).
fn keeps_text(name: &str) -> bool {
    matches!(name, "text" | "tspan" | "textPath" | "style")
}

/// The SVG rebuilt from its allowlisted content.
pub fn sanitize_svg(source: &[u8]) -> Result<Vec<u8>, SvgRejected> {
    if source.len() > MAX_SVG_SOURCE_BYTES {
        return Err(SvgRejected::TooLarge);
    }
    let text =
        std::str::from_utf8(source).map_err(|_| SvgRejected::Malformed("not UTF-8".into()))?;
    let text = without_doctype(text)?;
    let text = text.as_ref();
    // Bound the nesting before the parser sees it: a deeply nested document
    // must be refused, not allowed to exhaust the stack of the (possibly
    // small, app-owned) installing thread.
    if element_nesting_exceeds(text.as_bytes(), MAX_DEPTH) {
        return Err(SvgRejected::TooDeep);
    }
    let options = roxmltree::ParsingOptions {
        allow_dtd: false,
        nodes_limit: MAX_NODES,
    };
    let document = roxmltree::Document::parse_with_options(text, options)
        .map_err(|error| SvgRejected::Malformed(error.to_string()))?;
    for node in document.root().children() {
        if let Some(pi) = node.pi() {
            return Err(SvgRejected::Instruction(pi.target.to_owned()));
        }
    }
    let root = document.root_element();
    if root.tag_name().namespace() != Some(SVG_NS) || root.tag_name().name() != "svg" {
        return Err(SvgRejected::NotSvg);
    }
    let mut out = String::with_capacity(source.len().min(MAX_PACK_SVG_BYTES));
    write_element(root, 0, &mut out)?;
    if out.len() > MAX_PACK_SVG_BYTES {
        return Err(SvgRejected::TooLarge);
    }
    Ok(out.into_bytes())
}

/// Whether the markup nests elements deeper than `limit`, counted without
/// building a tree. Comments, CDATA, processing instructions and quoted
/// attribute values are skipped, so the count is exact for any document the
/// parser would accept; for anything else the parser refuses it anyway.
fn element_nesting_exceeds(markup: &[u8], limit: usize) -> bool {
    fn after(markup: &[u8], from: usize, end: &[u8]) -> usize {
        markup[from..]
            .windows(end.len())
            .position(|window| window == end)
            .map_or(markup.len(), |at| from + at + end.len())
    }
    let (mut depth, mut at) = (0usize, 0usize);
    while at < markup.len() {
        if markup[at] != b'<' {
            at += 1;
            continue;
        }
        let rest = &markup[at..];
        if rest.starts_with(b"<!--") {
            at = after(markup, at + 4, b"-->");
        } else if rest.starts_with(b"<![CDATA[") {
            at = after(markup, at + 9, b"]]>");
        } else if rest.starts_with(b"<?") {
            at = after(markup, at + 2, b"?>");
        } else if rest.starts_with(b"<!") {
            at = after(markup, at + 2, b">");
        } else if rest.starts_with(b"</") {
            depth = depth.saturating_sub(1);
            at = after(markup, at + 2, b">");
        } else {
            let mut quote = None;
            let mut end = at + 1;
            while end < markup.len() {
                match (quote, markup[end]) {
                    (None, b'"' | b'\'') => quote = Some(markup[end]),
                    (Some(open), byte) if byte == open => quote = None,
                    (None, b'>') => break,
                    _ => {}
                }
                end += 1;
            }
            if !(end > at + 1 && markup[end - 1] == b'/' && quote.is_none()) {
                depth += 1;
                if depth > limit + 1 {
                    return true;
                }
            }
            at = end + 1;
        }
    }
    false
}

/// Whether `bytes` are exactly what [`sanitize_svg`] makes of them.
pub fn is_clean_svg(bytes: &[u8]) -> bool {
    bytes.len() <= MAX_PACK_SVG_BYTES && sanitize_svg(bytes).is_ok_and(|clean| clean == bytes)
}

fn write_element(
    node: roxmltree::Node<'_, '_>,
    depth: usize,
    out: &mut String,
) -> Result<(), SvgRejected> {
    if depth > MAX_DEPTH {
        return Err(SvgRejected::TooDeep);
    }
    let tag = node.tag_name();
    let name = tag.name();
    let element = match tag.namespace() {
        Some(SVG_NS) if ELEMENTS.contains(&name) => {
            if never_drawn(name) {
                return Ok(());
            }
            if name == "a" {
                "g"
            } else {
                name
            }
        }
        Some(SVG_NS) => return Err(SvgRejected::Element(name.to_owned())),
        // Editor namespaces (Inkscape, Sodipodi, RDF metadata) are never
        // drawn; an unnamespaced or XHTML element is not SVG.
        Some(ns) if ns != "http://www.w3.org/1999/xhtml" => return Ok(()),
        _ => return Err(SvgRejected::Element(name.to_owned())),
    };
    out.push('<');
    out.push_str(element);
    if depth == 0 {
        let _ = write!(out, " xmlns=\"{SVG_NS}\" xmlns:xlink=\"{XLINK_NS}\"");
    }
    for attribute in node.attributes() {
        let value = attribute.value();
        let written = match attribute.namespace() {
            None => {
                let attr = attribute.name();
                if attr.len() > 64
                    || !attr
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                    || attr.to_ascii_lowercase().starts_with("on")
                {
                    return Err(SvgRejected::Attribute(attr.to_owned()));
                }
                if name == "a" && attr == "target" {
                    continue;
                }
                if attr == "href" {
                    if name == "a" {
                        continue;
                    }
                    check_local_reference(attr, value)?;
                } else if attr == "style" {
                    check_css(value)?;
                } else {
                    check_value(attr, value)?;
                }
                attr.to_owned()
            }
            Some(XLINK_NS) => {
                if attribute.name() != "href" {
                    continue;
                }
                if name == "a" {
                    continue;
                }
                check_local_reference("xlink:href", value)?;
                "xlink:href".to_owned()
            }
            Some(XML_NS) if matches!(attribute.name(), "space" | "lang") => {
                format!("xml:{}", attribute.name())
            }
            // Editor attributes (inkscape:label, sodipodi:nodetypes, …).
            Some(_) => continue,
        };
        out.push(' ');
        out.push_str(&written);
        out.push_str("=\"");
        escape(value, true, out);
        out.push('"');
    }
    let mut body = String::new();
    for child in node.children() {
        if child.is_element() {
            write_element(child, depth + 1, &mut body)?;
        } else if child.is_text() && keeps_text(name) {
            let text = child.text().unwrap_or_default();
            if name == "style" {
                check_css(text)?;
            }
            escape(text, false, &mut body);
        } else if let Some(pi) = child.pi() {
            return Err(SvgRejected::Instruction(pi.target.to_owned()));
        }
        // Comments and whitespace between elements are not drawn.
    }
    if body.is_empty() {
        out.push_str("/>");
    } else {
        out.push('>');
        out.push_str(&body);
        out.push_str("</");
        out.push_str(element);
        out.push('>');
    }
    Ok(())
}

/// Entities a DOCTYPE may declare, and how long each value may be.
const MAX_ENTITIES: usize = 64;
const MAX_ENTITY_VALUE: usize = 256;

/// `text` with its DOCTYPE removed and the plain text entities it declares
/// expanded. Any other declaration, a parameter or nested entity, or an
/// expansion beyond [`MAX_SVG_SOURCE_BYTES`] is refused.
fn without_doctype(text: &str) -> Result<std::borrow::Cow<'_, str>, SvgRejected> {
    let malformed = |why: &str| SvgRejected::Malformed(format!("DOCTYPE: {why}"));
    let Some(start) = text.find("<!DOCTYPE") else {
        return Ok(std::borrow::Cow::Borrowed(text));
    };
    let rest = &text[start + "<!DOCTYPE".len()..];
    let close = rest.find('>').ok_or_else(|| malformed("unterminated"))?;
    let (subset, end) = match rest.find('[') {
        Some(open) if open < close => {
            let subset_end = rest[open..]
                .find(']')
                .map(|at| open + at)
                .ok_or_else(|| malformed("unterminated subset"))?;
            let after = rest[subset_end + 1..].trim_start();
            if !after.starts_with('>') {
                return Err(malformed("unterminated"));
            }
            let end = rest.len() - after.len() + 1;
            (&rest[open + 1..subset_end], end)
        }
        _ => ("", close + 1),
    };
    let mut entities: Vec<(String, &str)> = Vec::new();
    let mut declarations = subset.trim();
    while !declarations.is_empty() {
        let body = declarations
            .strip_prefix("<!ENTITY")
            .ok_or_else(|| malformed("only entity declarations are allowed"))?
            .trim_start();
        let name_end = body
            .find(char::is_whitespace)
            .ok_or_else(|| malformed("entity"))?;
        let name = &body[..name_end];
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
        {
            return Err(malformed("entity name"));
        }
        let value_part = body[name_end..].trim_start();
        let quote = value_part
            .chars()
            .next()
            .filter(|c| *c == '"' || *c == '\'')
            .ok_or_else(|| malformed("entity value"))?;
        let value_end = value_part[1..]
            .find(quote)
            .ok_or_else(|| malformed("entity value"))?;
        let value = &value_part[1..1 + value_end];
        if value.len() > MAX_ENTITY_VALUE || value.contains(['&', '%', '<']) {
            return Err(malformed("entity value"));
        }
        declarations = value_part[value_end + 2..]
            .trim_start()
            .strip_prefix('>')
            .ok_or_else(|| malformed("entity"))?
            .trim_start();
        entities.push((format!("&{name};"), value));
        if entities.len() > MAX_ENTITIES {
            return Err(malformed("too many entities"));
        }
    }
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..start]);
    let mut body = &rest[end..];
    while let Some(at) = body.find('&') {
        out.push_str(&body[..at]);
        let reference = &body[at..];
        match entities
            .iter()
            .find(|(name, _)| reference.starts_with(name.as_str()))
        {
            Some((name, value)) => {
                out.push_str(value);
                body = &reference[name.len()..];
            }
            None => {
                out.push('&');
                body = &reference[1..];
            }
        }
        if out.len() > MAX_SVG_SOURCE_BYTES {
            return Err(SvgRejected::TooLarge);
        }
    }
    out.push_str(body);
    if out.len() > MAX_SVG_SOURCE_BYTES {
        return Err(SvgRejected::TooLarge);
    }
    Ok(std::borrow::Cow::Owned(out))
}

/// `href` must name an element of this file.
fn check_local_reference(attr: &str, value: &str) -> Result<(), SvgRejected> {
    let id = value.trim().strip_prefix('#');
    if id.is_some_and(|id| {
        !id.is_empty()
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
    }) {
        Ok(())
    } else {
        Err(SvgRejected::Attribute(attr.to_owned()))
    }
}

/// Every `url(` in a value must be `url(#id)`; no scheme may appear.
fn check_value(attr: &str, value: &str) -> Result<(), SvgRejected> {
    if only_local_urls(value) && !names_a_scheme(value) {
        Ok(())
    } else {
        Err(SvgRejected::Attribute(attr.to_owned()))
    }
}

fn check_css(css: &str) -> Result<(), SvgRejected> {
    let lower = css.to_ascii_lowercase();
    let unsafe_css = css.contains('\\')
        || lower.contains('@')
        || lower.contains("expression")
        || lower.contains("binding")
        || lower.contains("behavior")
        || !only_local_urls(css)
        || names_a_scheme(css);
    if unsafe_css {
        Err(SvgRejected::UnsafeCss)
    } else {
        Ok(())
    }
}

/// Each `url(` is followed (after spaces and an optional quote) by `#`.
fn only_local_urls(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let mut rest = lower.as_str();
    while let Some(at) = rest.find("url(") {
        let after = rest[at + 4..].trim_start_matches([' ', '\t', '\n', '\r', '\'', '"']);
        if !after.starts_with('#') {
            return false;
        }
        rest = &rest[at + 4..];
    }
    // `url (` with a space is not a URL token, but refuse the ambiguity.
    !lower.contains("url (") && !lower.contains("image(") && !lower.contains("image-set(")
}

fn names_a_scheme(value: &str) -> bool {
    let lower: String = value
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    [
        "javascript:",
        "data:",
        "http:",
        "https:",
        "file:",
        "vbscript:",
        "//",
    ]
    .iter()
    .any(|scheme| lower.contains(scheme))
}

fn escape(text: &str, attribute: bool, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            // Kept literally through attribute-value normalisation.
            '\t' if attribute => out.push_str("&#9;"),
            '\n' if attribute => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            _ => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(svg: &str) -> String {
        String::from_utf8(sanitize_svg(svg.as_bytes()).unwrap()).unwrap()
    }

    const HYPOTENUSE: &str = r##"<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<!-- Created with Inkscape -->
<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"
     xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape"
     xmlns:sodipodi="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd"
     xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
     width="250" height="200" viewBox="0 0 250 200" inkscape:version="1.2">
  <title>Right triangle</title>
  <metadata><rdf:RDF><rdf:Description about="x"/></rdf:RDF></metadata>
  <sodipodi:namedview pagecolor="#ffffff"/>
  <defs>
    <linearGradient id="g1"><stop offset="0" stop-color="#c00"/></linearGradient>
    <marker id="arrow"><path d="M0,0 L5,5"/></marker>
  </defs>
  <style>.label { font: 14px sans-serif; fill: url(#g1) } text > tspan { fill: #333 }</style>
  <a xlink:href="https://example.org/" target="_blank">
    <path d="M10,190 L240,190 L10,10 Z" fill="none" stroke="#000"
          marker-end="url(#arrow)" inkscape:label="triangle" sodipodi:nodetypes="ccc"/>
  </a>
  <use xlink:href="#arrow" x="5"/>
  <text x="120" y="110" class="label" xml:space="preserve">c <tspan font-style="italic">= √(a² + b²)</tspan></text>
  <switch><text systemLanguage="de" x="1" y="1">Hypotenuse</text><text x="1" y="1">Hypotenuse</text></switch>
</svg>
"##;

    #[test]
    fn a_drawing_keeps_everything_drawn_and_loses_editor_metadata() {
        let out = clean(HYPOTENUSE);
        assert!(out.starts_with(&format!(
            "<svg xmlns=\"{SVG_NS}\" xmlns:xlink=\"{XLINK_NS}\" width=\"250\" height=\"200\" viewBox=\"0 0 250 200\">"
        )), "{out}");
        for kept in [
            "<path d=\"M10,190 L240,190 L10,10 Z\" fill=\"none\" stroke=\"#000\" marker-end=\"url(#arrow)\"/>",
            "<use xlink:href=\"#arrow\" x=\"5\"/>",
            "<text x=\"120\" y=\"110\" class=\"label\" xml:space=\"preserve\">c <tspan font-style=\"italic\">= √(a² + b²)</tspan></text>",
            "<linearGradient id=\"g1\"><stop offset=\"0\" stop-color=\"#c00\"/></linearGradient>",
            "<style>.label { font: 14px sans-serif; fill: url(#g1) } text &gt; tspan { fill: #333 }</style>",
            "<switch><text systemLanguage=\"de\" x=\"1\" y=\"1\">Hypotenuse</text>",
        ] {
            assert!(out.contains(kept), "missing {kept} in {out}");
        }
        // The link is now a plain group.
        assert!(out.contains("<g><path"), "{out}");
        for gone in [
            "example.org",
            "target",
            "inkscape",
            "sodipodi",
            "rdf",
            "<title",
            "<metadata",
            "<!--",
            "<?xml",
        ] {
            assert!(!out.contains(gone), "{gone} left in {out}");
        }
        // The output is its own sanitised form, which the installer checks.
        assert!(is_clean_svg(out.as_bytes()));
        assert!(!is_clean_svg(HYPOTENUSE.as_bytes()));
    }

    fn rejected(body: &str) -> SvgRejected {
        let svg = format!("<svg xmlns=\"{SVG_NS}\" xmlns:xlink=\"{XLINK_NS}\">{body}</svg>");
        sanitize_svg(svg.as_bytes()).expect_err(body)
    }

    #[test]
    fn scripts_handlers_and_foreign_content_are_refused() {
        for body in [
            "<script>alert(1)</script>",
            "<foreignObject><div xmlns=\"http://www.w3.org/1999/xhtml\">x</div></foreignObject>",
            "<image xlink:href=\"data:image/png;base64,AAAA\"/>",
            "<animate attributeName=\"href\" to=\"javascript:alert(1)\"/>",
            "<set attributeName=\"fill\" to=\"red\"/>",
            "<iframe/>",
            "<html:div xmlns:html=\"http://www.w3.org/1999/xhtml\"/>",
            "<font-face/>",
        ] {
            assert!(matches!(rejected(body), SvgRejected::Element(_)), "{body}");
        }
        for body in [
            "<rect onload=\"alert(1)\"/>",
            "<rect ONCLICK=\"x()\"/>",
            "<use xlink:href=\"https://evil.example/s.svg#a\"/>",
            "<use href=\"other.svg#a\"/>",
            "<use xlink:href=\"#\"/>",
            "<rect fill=\"url(https://evil.example/p)\"/>",
            "<rect fill=\"url( 'http://x/' )\"/>",
            "<rect filter=\"url (http://x/)\"/>",
            "<rect fill=\"javascript:alert(1)\"/>",
            "<rect data=\"//evil.example/\"/>",
        ] {
            assert!(
                matches!(rejected(body), SvgRejected::Attribute(_)),
                "{body}"
            );
        }
        for body in [
            "<style>@import url(#x);</style>",
            "<style>rect { fill: url(http://x/) }</style>",
            "<style>rect { background: u\\72l(http://x/) }</style>",
            "<style>@font-face { src: local(x) }</style>",
            "<style>rect { fill: image(red) }</style>",
            "<rect style=\"fill:url(//evil.example/)\"/>",
            "<rect style=\"behavior: url(#x)\"/>",
        ] {
            assert_eq!(rejected(body), SvgRejected::UnsafeCss, "{body}");
        }
    }

    #[test]
    fn documents_that_are_not_plain_svg_are_refused() {
        for doctype in [
            // Nested entities (the "billion laughs" shape).
            "<!DOCTYPE svg [<!ENTITY a \"aaaa\"><!ENTITY b \"&a;&a;\">]>",
            "<!DOCTYPE svg [<!ENTITY % p \"x\">]>",
            "<!DOCTYPE svg [<!ENTITY e SYSTEM \"file:///etc/passwd\">]>",
            "<!DOCTYPE svg [<!ATTLIST rect fill CDATA \"red\">]>",
            "<!DOCTYPE svg [<!ELEMENT svg ANY>]>",
            "<!DOCTYPE svg [<!ENTITY a \"<script/>\">]>",
        ] {
            let svg = format!("{doctype}<svg xmlns=\"{SVG_NS}\"><text>&a;</text></svg>");
            assert!(
                matches!(sanitize_svg(svg.as_bytes()), Err(SvgRejected::Malformed(_))),
                "{doctype}"
            );
        }
        // Many references to a declared entity cannot grow past the bound.
        let wide = format!(
            "<!DOCTYPE svg [<!ENTITY w \"{}\">]><svg xmlns=\"{SVG_NS}\"><text>{}</text></svg>",
            "w".repeat(MAX_ENTITY_VALUE),
            "&w;".repeat(MAX_SVG_SOURCE_BYTES / MAX_ENTITY_VALUE + 2)
        );
        assert_eq!(sanitize_svg(wide.as_bytes()), Err(SvgRejected::TooLarge));
        let stylesheet = "<?xml-stylesheet href=\"http://x/s.css\"?>\
                          <svg xmlns=\"http://www.w3.org/2000/svg\"/>";
        assert_eq!(
            sanitize_svg(stylesheet.as_bytes()),
            Err(SvgRejected::Instruction("xml-stylesheet".into()))
        );
        assert_eq!(
            sanitize_svg(b"<svg><rect/></svg>"),
            Err(SvgRejected::NotSvg),
            "no SVG namespace"
        );
        assert_eq!(
            sanitize_svg(b"<html xmlns=\"http://www.w3.org/2000/svg\"/>"),
            Err(SvgRejected::NotSvg)
        );
        assert!(matches!(
            sanitize_svg(b"\xff\xfe<svg/>"),
            Err(SvgRejected::Malformed(_))
        ));
        let deep = format!(
            "<svg xmlns=\"{SVG_NS}\">{}{}</svg>",
            "<g>".repeat(MAX_DEPTH + 2),
            "</g>".repeat(MAX_DEPTH + 2)
        );
        assert_eq!(sanitize_svg(deep.as_bytes()), Err(SvgRejected::TooDeep));
        // TRUST-003: far deeper nesting is refused before parsing; it used to
        // overflow the installing thread's stack.
        for depth in [800, 5_000, 200_000] {
            let bomb = format!(
                "<svg xmlns=\"{SVG_NS}\">{}{}</svg>",
                "<g>".repeat(depth),
                "</g>".repeat(depth)
            );
            assert_eq!(sanitize_svg(bomb.as_bytes()), Err(SvgRejected::TooDeep));
            let unclosed = format!("<svg xmlns=\"{SVG_NS}\">{}", "<g>".repeat(depth));
            assert_eq!(sanitize_svg(unclosed.as_bytes()), Err(SvgRejected::TooDeep));
        }
        // Comments, CDATA and quoted attribute text that merely look like markup
        // do not count as nesting.
        let decoys = format!(
            "<svg xmlns=\"{SVG_NS}\"><!-- {} --><desc><![CDATA[{}]]></desc><g id=\"a>b\" class='c>d'/></svg>",
            "<g>".repeat(500),
            "<g>".repeat(500)
        );
        assert!(sanitize_svg(decoys.as_bytes()).is_ok());
        let huge = vec![b' '; MAX_SVG_SOURCE_BYTES + 1];
        assert_eq!(sanitize_svg(&huge), Err(SvgRejected::TooLarge));
    }

    #[test]
    fn a_plain_doctype_and_simple_entities_are_expanded_and_dropped() {
        // As written by Adobe Illustrator and by many SVG 1.1 tools.
        let illustrator = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
            <!-- Generator: Adobe Illustrator 12.0.0 -->\n\
            <!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\" [\n\
            \t<!ENTITY ns_svg \"http://www.w3.org/2000/svg\">\n\
            \t<!ENTITY ns_xlink 'http://www.w3.org/1999/xlink'>\n\
            ]>\n\
            <svg version=\"1.1\" xmlns=\"&ns_svg;\" xmlns:xlink=\"&ns_xlink;\"><rect width=\"1\"/>\
            <text>A &amp; B</text></svg>";
        assert_eq!(
            clean(illustrator),
            format!(
                "<svg xmlns=\"{SVG_NS}\" xmlns:xlink=\"{XLINK_NS}\" version=\"1.1\"><rect width=\"1\"/>\
                 <text>A &amp; B</text></svg>"
            )
        );
        let plain = "<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"x.dtd\">\
                     <svg xmlns=\"http://www.w3.org/2000/svg\"><circle r=\"2\"/></svg>";
        assert!(clean(plain).ends_with("><circle r=\"2\"/></svg>"));
        // Inkscape flowed text is drawn by neither browsers nor Wikimedia.
        let flowed = format!(
            "<svg xmlns=\"{SVG_NS}\"><flowRoot><flowRegion><rect/></flowRegion>\
             <flowPara>x</flowPara></flowRoot><circle r=\"1\"/></svg>"
        );
        assert!(clean(&flowed).ends_with("\"><circle r=\"1\"/></svg>"));
    }

    #[test]
    fn attribute_values_survive_a_round_trip_exactly() {
        let svg = format!(
            "<svg xmlns=\"{SVG_NS}\"><text x=\"1\" font-family=\"'A &amp; B', &quot;C&quot;\" \
             data-n=\"a&#9;b&#10;c\">1 &lt; 2 &amp;&amp; 3 &gt; 2</text></svg>"
        );
        let out = clean(&svg);
        assert!(
            out.contains("font-family=\"'A &amp; B', &quot;C&quot;\""),
            "{out}"
        );
        assert!(out.contains("data-n=\"a&#9;b&#10;c\""), "{out}");
        assert!(
            out.contains(">1 &lt; 2 &amp;&amp; 3 &gt; 2</text>"),
            "{out}"
        );
        assert!(is_clean_svg(out.as_bytes()));
        // Any change to a clean file makes it unclean.
        let spaced = out.replacen("<text", "<text ", 1);
        assert!(!is_clean_svg(spaced.as_bytes()));
    }
}
