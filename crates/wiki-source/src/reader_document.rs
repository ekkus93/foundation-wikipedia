//! Reader-ready HTML documents for an embedded article WebView (MOB-002).
//!
//! Article HTML from a pack (especially an unsigned custom one) is untrusted,
//! so it is sanitized again here, whatever the source claims, before display:
//! no scripts, styles, event handlers, forms, frames or `javascript:` URLs.
//! Links are rewritten for the app's link interception:
//!
//! * links to other Wikipedia articles (`./Title`, `/wiki/Title`) become
//!   `https://foundation.invalid/wiki/Title`, which the app resolves within
//!   the same offline pack;
//! * in-page fragments (`#section`) are kept;
//! * other relative URLs are removed, so nothing resolves against the origin;
//! * absolute `http(s)` links are kept for the app to treat as external;
//! * image sources are removed, except images the pack itself holds
//!   (PACK-003): those point at the app's local image URL
//!   ([`READER_IMAGE_BASE`] here, or the desktop's own scheme), by SHA-256.
//!
//! The document is wrapped with Wikipedia-like styling and a strict Content
//! Security Policy. `foundation.invalid` is a reserved name that never
//! resolves; the app also refuses every subresource request, so nothing
//! leaves the device. Offline media is served later through the same origin.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use ammonia::{Builder, UrlRelative};
use wiki_model::{Article, MediaDerivation};

/// The origin article documents are loaded at; never a real host.
pub const READER_ORIGIN: &str = "https://foundation.invalid";
/// Path prefix of in-app article links.
pub const ARTICLE_PATH: &str = "/wiki/";

const MAX_TITLE_BYTES: usize = 512;
/// Element IDs the app reserves; stripped from article HTML.
const RESERVED_ID_PREFIX: &str = "foundation-";
/// Fragment of the cited passage in a [`reader_document_with_citation`].
pub const CITED_FRAGMENT: &str = "foundation-cited";

/// Colours for the document; the app passes its current theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReaderTheme {
    Light,
    Dark,
}

/// Rewrite one relative URL from article HTML (see the module docs).
fn rewrite_relative(url: &str) -> Option<Cow<'_, str>> {
    if url.starts_with('#') {
        return Some(Cow::Borrowed(url));
    }
    let title = url
        .strip_prefix("./")
        .or_else(|| url.strip_prefix(ARTICLE_PATH))?;
    if title.is_empty()
        || title.len() > MAX_TITLE_BYTES
        || title.starts_with('/')
        || title.contains("..")
        || title.contains('?')
        || title.bytes().any(|b| b.is_ascii_control() || b == b'\\')
    {
        return None;
    }
    Some(Cow::Owned(format!("{READER_ORIGIN}{ARTICLE_PATH}{title}")))
}

/// Where Android's reader loads a pack image: `<base><sha256>`, answered by
/// the app from the installed pack.
pub const READER_IMAGE_BASE: &str = "https://foundation.invalid/media/";

/// The images a pack holds for one article: the local URL base and the
/// SHA-256 of each Wikimedia file name (`Hypotenuse.svg`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackImageSources {
    pub base: String,
    pub by_file: HashMap<String, String>,
    /// SHA-256 of each image that is the first frame of an animation
    /// ([`MediaDerivation::FirstFrameStill`]); each is labelled in the page.
    pub stills: HashSet<String>,
}

/// Shown after an animation the pack holds only as its first frame.
pub const STILL_IMAGE_NOTE: &str = "Still image: the animation is online only.";

impl PackImageSources {
    /// The images `article` holds, served under `base`.
    pub fn for_article(article: &Article, base: &str) -> Self {
        Self {
            base: base.to_owned(),
            by_file: article
                .media
                .iter()
                .filter(|asset| asset.derivation != Some(MediaDerivation::RenderedMap))
                .filter_map(|asset| {
                    Some((
                        crate::media_download::media_file_name(&asset.source_url)?,
                        asset.sha256.clone()?,
                    ))
                })
                // Drawn maps, in document order (`map_data::scan_maps`).
                .chain(
                    article
                        .media
                        .iter()
                        .filter(|asset| asset.derivation == Some(MediaDerivation::RenderedMap))
                        .enumerate()
                        .filter_map(|(n, asset)| Some((map_image_key(n), asset.sha256.clone()?))),
                )
                .collect(),
            stills: article
                .media
                .iter()
                .filter(|asset| asset.derivation == Some(MediaDerivation::FirstFrameStill))
                .filter_map(|asset| asset.sha256.clone())
                .collect(),
        }
    }
}

/// Sanitize untrusted article HTML for display; no image is loaded.
pub fn sanitize_article_html(html: &str) -> String {
    sanitize_with_images(html, None)
}

/// Sanitize a pack article, pointing each image the pack holds at its local
/// URL. Other image sources are removed, and no other attribute may use the
/// local image URL.
pub fn sanitize_pack_article_html(html: &str, images: &PackImageSources) -> String {
    sanitize_with_images(html, Some(images))
}

/// Shown instead of an article whose HTML is beyond the limits above.
pub const TOO_COMPLEX_NOTE: &str =
    "This article is too large or too deeply nested to display safely.";

fn too_complex() -> String {
    format!("<p class=\"foundation-too-complex\">{TOO_COMPLEX_NOTE}</p>")
}

fn sanitize_with_images(html: &str, images: Option<&PackImageSources>) -> String {
    if !wiki_model::html_within_display_limits(html) {
        return too_complex();
    }
    let base_scheme = images
        .and_then(|images| images.base.split_once("://"))
        .map(|(scheme, _)| scheme.to_ascii_lowercase());
    let filter_scheme = base_scheme.clone();
    let mut builder = Builder::default();
    let mut schemes = HashSet::from(["http", "https"]);
    if let Some(scheme) = base_scheme.as_deref() {
        schemes.insert(scheme);
    }
    let images = images.cloned();
    let label = images.clone();
    builder
        .url_schemes(schemes)
        .url_relative(UrlRelative::Custom(Box::new(rewrite_relative)))
        // Keep heading/citation anchors and Wikipedia's presentational classes;
        // with scripts and styles removed they cannot run or restyle the app.
        .add_generic_attributes(["id", "class"])
        // srcset could name other files; only the mapped src is kept.
        .rm_tag_attributes("img", ["srcset"])
        // Formulas are text (presentation MathML), so they work offline;
        // Wikipedia renders math only as MathML (SRC-003).
        .add_tags(MATHML_TAGS)
        .add_generic_attributes(MATHML_ATTRIBUTES)
        .add_clean_content_tags(["annotation", "annotation-xml"])
        .id_prefix(None)
        // The app's own anchors must not be claimed by article markup.
        .attribute_filter(move |element, attribute, value| {
            if attribute == "id" && value.starts_with(RESERVED_ID_PREFIX) {
                return None;
            }
            if element == "img" && attribute == "src" {
                // Only an image this pack holds is ever loaded.
                let images = images.as_ref()?;
                if let Some(n) = value
                    .strip_prefix(MAP_IMAGE_SOURCE)
                    .and_then(|n| n.parse::<usize>().ok())
                {
                    let digest = images.by_file.get(&map_image_key(n))?;
                    return Some(format!("{}{digest}", images.base).into());
                }
                let digest = images
                    .by_file
                    .get(&crate::media_download::media_file_name(value)?)?;
                return Some(format!("{}{digest}", images.base).into());
            }
            // Local image URLs are the app's, never the article's.
            let lower = value.trim().to_ascii_lowercase();
            let local = lower.starts_with(READER_IMAGE_BASE)
                || images
                    .as_ref()
                    .is_some_and(|images| lower.starts_with(&images.base.to_ascii_lowercase()))
                || filter_scheme
                    .as_deref()
                    .is_some_and(|scheme| scheme != "https" && lower.starts_with(scheme));
            if local {
                return None;
            }
            Some(value.into())
        })
        .link_rel(Some("noopener noreferrer nofollow"));
    let clean = builder
        .clean(&without_interactive_maps(
            &crate::av_preview::with_av_previews(html),
            label.as_ref(),
        ))
        .to_string();
    match &label {
        Some(images) if !images.stills.is_empty() => label_stills(clean, images),
        _ => clean,
    }
}

/// Insert [`STILL_IMAGE_NOTE`] after each image whose source is a still.
/// Runs on sanitized output, where every attribute value is double-quoted
/// with `"` escaped, so the first `>` outside quotes ends the tag.
fn label_stills(html: String, images: &PackImageSources) -> String {
    let note = format!("<span class=\"foundation-still-note\">{STILL_IMAGE_NOTE}</span>");
    let sources: Vec<String> = images
        .stills
        .iter()
        .map(|digest| format!(" src=\"{}{digest}\"", images.base))
        .collect();
    let mut out = String::with_capacity(html.len());
    let mut rest = html.as_str();
    while let Some(start) = rest.find("<img ") {
        let mut quoted = false;
        let Some(end) = rest[start..]
            .char_indices()
            .find(|(_, c)| {
                if *c == '"' {
                    quoted = !quoted;
                }
                *c == '>' && !quoted
            })
            .map(|(at, _)| start + at + 1)
        else {
            break;
        };
        out.push_str(&rest[..end]);
        if sources
            .iter()
            .any(|source| rest[start..end].contains(source))
        {
            out.push_str(&note);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// Image hosts an online article may load from (DUI-001): Wikimedia's
/// media store and its math-rendering service. Protocol-relative sources
/// become https; any other source is removed (alt text remains).
const ONLINE_IMAGE_PREFIXES: [&str; 3] = [
    "https://upload.wikimedia.org/",
    "https://thumb.wikimedia.org/",
    "https://wikimedia.org/api/rest_v1/media/math/render/",
];

/// Presentation MathML elements an online article may use (no scripting,
/// links or external resources exist in this set).
pub(crate) const MATHML_TAGS: [&str; 26] = [
    "math",
    "semantics",
    "mrow",
    "mi",
    "mn",
    "mo",
    "ms",
    "mtext",
    "mspace",
    "msup",
    "msub",
    "msubsup",
    "mfrac",
    "msqrt",
    "mroot",
    "mstyle",
    "mpadded",
    "mphantom",
    "menclose",
    "mover",
    "munder",
    "munderover",
    "mtable",
    "mtr",
    "mtd",
    "mmultiscripts",
];

/// Layout-only MathML attributes (none take URLs or script).
pub(crate) const MATHML_ATTRIBUTES: [&str; 22] = [
    "display",
    "displaystyle",
    "scriptlevel",
    "mathvariant",
    "stretchy",
    "fence",
    "separator",
    "accent",
    "accentunder",
    "lspace",
    "rspace",
    "linethickness",
    "columnalign",
    "rowalign",
    "columnspacing",
    "rowspacing",
    "notation",
    "minsize",
    "maxsize",
    "movablelimits",
    "symmetric",
    "largeop",
];

/// Shown where an article has an interactive (Kartographer) map that the
/// pack does not hold drawn (or in an online article).
pub const MAP_NOTE: &str = "Map (online only)";

/// The [`PackImageSources::by_file`] key of an article's `n`-th drawn map.
pub fn map_image_key(n: usize) -> String {
    format!("map:{n}")
}

/// Under each map drawn from open data. The exact credit (Natural Earth,
/// and "© OpenStreetMap contributors" where its data is drawn) is in the
/// map itself and in its media licence.
pub const MAP_CREDIT: &str = "Map drawn offline from open map data (credits on the map)";

/// Show each interactive map as the pack's drawn copy, or as [`MAP_NOTE`]
/// with its centre and a link to view it online; unlink inline map links
/// (PACK-003).
///
/// Kartographer maps are drawn by Wikimedia Maps (`maps.wikimedia.org`),
/// whose terms forbid third-party apps and downloading maps for offline
/// use, so their images are never loaded. The pack builder draws each map
/// itself from open data (`map_data`), stored like any pack image under
/// [`map_image_key`]. Runs before sanitising; what it writes is sanitised
/// too, and the drawn map's source is resolved by the image filter.
fn without_interactive_maps(html: &str, images: Option<&PackImageSources>) -> String {
    // Stored articles carry the notes this sanitiser wrote; renaming any
    // the article itself contains keeps the n-th note the n-th map.
    let html = if images.is_none() {
        html.replace(MAP_NOTE_CLASS, "foundation-map-note-text")
    } else {
        html.to_owned()
    };
    let spans = crate::map_data::scan_maps(&html);
    if spans.is_empty() {
        return match images {
            Some(images) => with_drawn_maps(&html, images),
            None => html,
        };
    }
    // Rendered HTML (or a pack made before notes were stored).
    let mut out = String::with_capacity(html.len());
    let mut at = 0;
    let mut frame = 0;
    for span in spans {
        match span {
            crate::map_data::MapSpan::Frame { start, end, tag } => {
                out.push_str(&html[at..start]);
                match images.filter(|images| images.by_file.contains_key(&map_image_key(frame))) {
                    Some(_) => out.push_str(&drawn_map(frame)),
                    None => out.push_str(&map_note(tag)),
                }
                frame += 1;
                at = end;
            }
            crate::map_data::MapSpan::Link { start, end, text } => {
                // Keep the link text; the map page it opens is not here.
                out.push_str(&html[at..start]);
                out.push_str(text);
                at = end;
            }
        }
    }
    out.push_str(&html[at..]);
    out
}

const MAP_NOTE_CLASS: &str = "foundation-map-note";

/// The pack's drawn copy of map `n`, with its credit.
fn drawn_map(n: usize) -> String {
    format!(
        "<img src=\"{MAP_IMAGE_SOURCE}{n}\" alt=\"Map\">\
         <span class=\"foundation-map-credit\">{MAP_CREDIT}</span>"
    )
}

/// Replace the n-th stored map note with the pack's drawn map `n`.
fn with_drawn_maps(html: &str, images: &PackImageSources) -> String {
    let open = format!("<span class=\"{MAP_NOTE_CLASS}\">");
    if !html.contains(&open) {
        return html.to_owned();
    }
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    let mut n = 0;
    while let Some(start) = rest.find(&open) {
        // A note holds no nested span.
        let Some(close) = rest[start..].find("</span>").map(|at| start + at) else {
            break;
        };
        out.push_str(&rest[..start]);
        if images.by_file.contains_key(&map_image_key(n)) {
            out.push_str(&drawn_map(n));
        } else {
            out.push_str(&rest[start..close + "</span>".len()]);
        }
        rest = &rest[close + "</span>".len()..];
        n += 1;
    }
    out.push_str(rest);
    out
}

/// The source the map rewrite gives a drawn map, resolved by the image
/// filter to the pack's local URL (and removed where there is none); an
/// `https` URL, as the sanitiser drops other schemes before filtering.
const MAP_IMAGE_SOURCE: &str = "https://foundation.invalid/map/";

fn map_note(tag: &str) -> String {
    let map = crate::map_data::map_frame(tag);
    let zoom = map.zoom.unwrap_or(12);
    let mut note = format!("<span class=\"foundation-map-note\">{MAP_NOTE}");
    if let (Some(lat), Some(lon)) = (map.lat, map.lon) {
        let (lat_text, lon_text) = (trimmed(lat), trimmed(lon));
        let north = if lat < 0.0 { "S" } else { "N" };
        let east = if lon < 0.0 { "W" } else { "E" };
        note.push_str(&format!(
            " · {}° {north}, {}° {east} · <a href=\"https://www.openstreetmap.org/?mlat={lat_text}&amp;mlon={lon_text}#map={zoom}/{lat_text}/{lon_text}\">View on OpenStreetMap</a>",
            trimmed(lat.abs()),
            trimmed(lon.abs()),
        ));
    }
    note.push_str("</span>");
    note
}

/// At most five decimals, without trailing zeros.
fn trimmed(value: f64) -> String {
    let text = format!("{value:.5}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn online_image_source(value: &str) -> Option<String> {
    let absolute = match value.strip_prefix("//") {
        Some(rest) => format!("https://{rest}"),
        None => value.to_owned(),
    };
    let safe = absolute.len() <= 2048
        && absolute
            .bytes()
            .all(|b| b.is_ascii_graphic() && !matches!(b, b'"' | b'\\' | b'<' | b'>' | b'`'));
    (safe
        && ONLINE_IMAGE_PREFIXES
            .iter()
            .any(|prefix| absolute.starts_with(prefix)))
    .then_some(absolute)
}

/// Sanitize live rendered HTML (MediaWiki `action=parse`) for the desktop's
/// online reader. Like [`sanitize_article_html`] it removes scripts, styles,
/// event handlers and forms and keeps `id`/`class` for Wikipedia-like
/// styling, but it keeps article links as they are (the app validates each
/// clicked `href` itself), keeps image sources only from Wikimedia's media
/// and math hosts, and keeps presentation MathML (without its TeX
/// annotation) because Wikipedia now sends formulas only as MathML.
pub fn sanitize_online_article_html(html: &str) -> String {
    if !wiki_model::html_within_display_limits(html) {
        return too_complex();
    }
    let mut builder = Builder::default();
    builder
        .url_schemes(HashSet::from(["http", "https"]))
        .add_generic_attributes(["id", "class"])
        .rm_tag_attributes("img", ["srcset"])
        // Wikipedia renders formulas as native MathML (no fallback image),
        // so presentation MathML is kept; its TeX annotation is dropped.
        .add_tags(MATHML_TAGS)
        .add_generic_attributes(MATHML_ATTRIBUTES)
        .add_clean_content_tags(["annotation", "annotation-xml"])
        .id_prefix(None)
        .attribute_filter(|element, attribute, value| match (element, attribute) {
            (_, "id") if value.starts_with(RESERVED_ID_PREFIX) => None,
            ("img", "src") => online_image_source(value).map(Cow::Owned),
            _ => Some(value.into()),
        })
        .link_rel(Some("noopener noreferrer nofollow"));
    builder
        .clean(&without_interactive_maps(
            &crate::av_preview::with_av_previews(html),
            None,
        ))
        .to_string()
}

pub(crate) fn escape_text(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            c if c.is_control() => {}
            c => escaped.push(c),
        }
    }
    escaped
}

const STYLE: &str = "\
:root{--bg:#fff;--fg:#202122;--muted:#54595d;--rule:#a2a9b1;--link:#3366cc;--box:#f8f9fa;--cite:#fef6e7}\
:root.dark{--bg:#101418;--fg:#eaecf0;--muted:#a2a9b1;--rule:#54595d;--link:#88a3e8;--box:#1b1f24;--cite:#3b3220}\
html{background:var(--bg);color:var(--fg)}\
body{margin:0 auto;max-width:46rem;padding:12px 16px 96px;font:17px/1.6 sans-serif;overflow-wrap:break-word}\
h1,h2{font-family:Georgia,'Linux Libertine',serif;font-weight:400;border-bottom:1px solid var(--rule)}\
h1{font-size:1.75em;margin:.4em 0 .2em}h2{font-size:1.4em;margin-top:1.2em}\
h3{font-size:1.15em}\
a{color:var(--link);text-decoration:none}\
.revision{color:var(--muted);font-size:.8em;margin:0 0 1em}\
table{display:block;overflow-x:auto;border-collapse:collapse;max-width:100%}\
td,th{border:1px solid var(--rule);padding:.2em .4em}\
img{max-width:100%;height:auto}\
blockquote,pre{background:var(--box);padding:.5em .8em;overflow-x:auto}\
sup{line-height:1}\
.foundation-cited{background:var(--cite);border-left:4px solid var(--link);padding:.05em .7em;margin:0 -.7em;scroll-margin-top:16px}\
.foundation-cited-note{color:var(--muted);font-size:.8em;margin:0}\
.foundation-still-note{display:block;color:var(--muted);font-size:.8em}\
.foundation-map-note{display:block;border:1px solid var(--rule);background:var(--box);padding:.6em .8em;color:var(--muted);font-size:.9em}\
.foundation-map-credit{display:block;color:var(--muted);font-size:.75em}\
.foundation-too-complex{border:1px solid var(--rule);background:var(--box);padding:.6em .8em;color:var(--muted)}\
.foundation-attribution{border-top:1px solid var(--rule);margin-top:2em;font-size:.85em;color:var(--muted)}\
.foundation-attribution h2{font-size:1.1em}.foundation-attribution h3{font-size:1em}\
.foundation-attribution-note{font-size:.9em}\
.foundation-av-note{display:block;border:1px solid var(--rule);background:var(--box);padding:.4em .7em;color:var(--muted);font-size:.9em}";

/// A complete, script-free document for one verified article revision.
pub fn reader_document(article: &Article, theme: ReaderTheme) -> String {
    wrap_document(
        article,
        theme,
        &sanitize_pack_article_html(
            &article.rendered_html,
            &PackImageSources::for_article(article, READER_IMAGE_BASE),
        ),
    )
}

/// How a cited passage was marked in a [`reader_document_with_citation`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CitedPassage {
    /// The paragraph in the article text is highlighted.
    Located,
    /// The passage could not be matched to exactly one paragraph of the
    /// rendered article, so its text is shown, labelled, at the top.
    ShownSeparately,
}

/// A reader document with the cited evidence block highlighted at
/// [`CITED_FRAGMENT`]. `None` if `block_id` is not a block of this exact
/// article revision (a stale or foreign citation is never highlighted).
///
/// Evidence blocks come from structured content while the page shows the
/// rendered HTML, so the block is located by its exact normalized text, and
/// only when exactly one paragraph matches; otherwise it is never guessed.
pub fn reader_document_with_citation(
    article: &Article,
    theme: ReaderTheme,
    block_id: &str,
) -> Option<(String, CitedPassage)> {
    let blocks = wiki_core::evidence::evidence_blocks(article).ok()?;
    let block = blocks.iter().find(|entry| entry.id == block_id)?;
    let body = sanitize_pack_article_html(
        &article.rendered_html,
        &PackImageSources::for_article(article, READER_IMAGE_BASE),
    );
    let text = block_text(&block.block.content);
    if let Some((start, end)) = text
        .as_deref()
        .and_then(|text| locate_paragraph(&body, text))
    {
        let marked = format!(
            "{}<div id=\"{CITED_FRAGMENT}\" class=\"foundation-cited\">{}</div>{}",
            &body[..start],
            &body[start..end],
            &body[end..]
        );
        return Some((
            wrap_document(article, theme, &marked),
            CitedPassage::Located,
        ));
    }
    let heading = if block.headings.is_empty() {
        "Introduction".to_owned()
    } else {
        block.headings.join(" › ")
    };
    let excerpt = text.unwrap_or_else(|| fallback_text(&block.block.content));
    let note = format!(
        "<div id=\"{CITED_FRAGMENT}\" class=\"foundation-cited\">\
<p class=\"foundation-cited-note\">Cited passage ({}). Its exact place in this page \
could not be found.</p><p>{}</p></div>{body}",
        escape_text(&heading),
        escape_text(&excerpt)
    );
    Some((
        wrap_document(article, theme, &note),
        CitedPassage::ShownSeparately,
    ))
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn block_text(content: &wiki_model::BlockContent) -> Option<String> {
    match content {
        wiki_model::BlockContent::Paragraph(text) | wiki_model::BlockContent::Quote(text) => {
            Some(normalize(text)).filter(|text| !text.is_empty())
        }
        _ => None,
    }
}

/// Plain text of a non-paragraph block, for the separately shown passage.
fn fallback_text(content: &wiki_model::BlockContent) -> String {
    use wiki_model::BlockContent;
    let text = match content {
        BlockContent::Paragraph(text) | BlockContent::Quote(text) => text.clone(),
        BlockContent::List(items) => items.join(" · "),
        BlockContent::Table(rows) => rows
            .iter()
            .map(|row| row.join(" | "))
            .collect::<Vec<_>>()
            .join(" / "),
        BlockContent::Infobox(rows) => rows
            .iter()
            .map(|(key, value)| format!("{key}: {value}"))
            .collect::<Vec<_>>()
            .join(" · "),
        BlockContent::Footnote(note) => note.text.clone(),
        BlockContent::Math { source, .. } => source.clone(),
        BlockContent::Media { .. } => "(media)".to_owned(),
        BlockContent::HtmlFallback(_) => "(formatted content)".to_owned(),
    };
    normalize(&text)
}

/// Decode the entities the html5ever serializer emits in text.
fn decode_text(text: &str, out: &mut String) {
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let (decoded, len) = [
            ("&amp;", '&'),
            ("&lt;", '<'),
            ("&gt;", '>'),
            ("&quot;", '"'),
            ("&nbsp;", '\u{a0}'),
            ("&#39;", '\''),
        ]
        .iter()
        .find(|(entity, _)| rest.starts_with(entity))
        .map_or(('&', 1), |(entity, c)| (*c, entity.len()));
        out.push(decoded);
        rest = &rest[len..];
    }
    out.push_str(rest);
}

/// End of the tag starting at `start` (a `<`), honouring quoted attributes.
fn tag_end(html: &str, start: usize) -> Option<usize> {
    let mut quote = None;
    for (offset, c) in html[start..].char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '>') => return Some(start + offset + 1),
            _ => {}
        }
    }
    None
}

fn tag_name(tag: &str) -> (bool, &str) {
    let inner = tag.trim_start_matches('<');
    let (closing, inner) = match inner.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (false, inner),
    };
    let end = inner
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(inner.len());
    (closing, &inner[..end])
}

/// Byte range of the one `<p>` element of sanitized `html` whose text (with
/// or without superscripts such as reference markers) is exactly `text`.
fn locate_paragraph(html: &str, text: &str) -> Option<(usize, usize)> {
    let mut found = None;
    let mut at = 0;
    // (start, full text, text without <sup> content, sup depth)
    let mut open: Option<(usize, String, String, usize)> = None;
    while at < html.len() {
        let Some(lt) = html[at..].find('<').map(|offset| at + offset) else {
            break;
        };
        if let Some((_, full, bare, sup)) = open.as_mut() {
            decode_text(&html[at..lt], full);
            if *sup == 0 {
                decode_text(&html[at..lt], bare);
            }
        }
        let end = tag_end(html, lt)?;
        let (closing, name) = tag_name(&html[lt..end]);
        match (closing, name, open.as_mut()) {
            (false, "p", _) => open = Some((lt, String::new(), String::new(), 0)),
            (false, "sup", Some((_, _, _, sup))) => *sup += 1,
            (true, "sup", Some((_, _, _, sup))) => *sup = sup.saturating_sub(1),
            (true, "p", Some(_)) => {
                let (start, full, bare, _) = open.take()?;
                if normalize(&full) == text || normalize(&bare) == text {
                    if found.is_some() {
                        return None; // ambiguous: never guess
                    }
                    found = Some((start, end));
                }
            }
            _ => {}
        }
        at = end;
    }
    found
}

fn wrap_document(article: &Article, theme: ReaderTheme, body: &str) -> String {
    let class = match theme {
        ReaderTheme::Light => "",
        ReaderTheme::Dark => " class=\"dark\"",
    };
    let lang = if article
        .language
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && !article.language.is_empty()
        && article.language.len() <= 32
    {
        article.language.as_str()
    } else {
        "en"
    };
    format!(
        "<!doctype html><html lang=\"{lang}\"{class}><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; img-src {READER_ORIGIN}\">\
<meta name=\"color-scheme\" content=\"{scheme}\">\
<title>{title}</title><style>{STYLE}</style></head><body>\
<h1>{title}</h1><p class=\"revision\">Revision {revision} · offline copy</p>\
<main>{body}</main>{attribution}</body></html>",
        scheme = match theme {
            ReaderTheme::Light => "light",
            ReaderTheme::Dark => "dark",
        },
        title = escape_text(&article.display_title),
        revision = article.revision.revision_id,
        attribution = crate::attribution::attribution_section(article),
    )
}

#[cfg(test)]
mod tests {

    /// Excerpts of real enwiki mapframes (Tokyo rev 1386294178, Eiffel
    /// Tower rev 1385997089): one with a stated centre, one whose centre
    /// comes from its shapes, and an inline map link.
    const TOKYO_MAP: &str = "<figure class=\"mw-halign-right\" typeof=\"mw:File/Frame\"><a class=\"mw-kartographer-map notheme noviewer mw-file-description mw-kartographer-container center\" style=\"min-width: 300px; min-height: 200px;\" data-mw-kartographer=\"mapframe\" data-style=\"osm-intl\" data-width=\"300\" data-height=\"200\" data-zoom=\"8\" data-lat=\"35.68\" data-lon=\"139.41\" data-overlays=\"[&quot;_a5ac78316acb3ce528070585824e3c65cae30b9f&quot;]\" href=\"/wiki/Special:Map/8/35.68/139.41/en\"><img class=\"mw-file-element\" src=\"https://maps.wikimedia.org/img/osm-intl,8,35.68,139.41,300x200.png?lang=en&amp;domain=en.wikipedia.org&amp;title=Tokyo&amp;revid=1386294178&amp;groups=_a5ac78316acb3ce528070585824e3c65cae30b9f&amp;parser=legacy\" width=\"300\" height=\"200\" decoding=\"async\"></a><figcaption>Map of Tokyo</figcaption></figure>";
    const EIFFEL_MAP: &str = "<a class=\"mw-kartographer-map notheme noviewer mw-file-description mw-kartographer-container center\" style=\"min-width: 250px; min-height: 200px;\" data-mw-kartographer=\"mapframe\" data-style=\"osm-intl\" data-width=\"250\" data-height=\"200\" data-zoom=\"16\" data-overlays=\"[&quot;_c391d58e02980fe061617195986043f32c6c3bcd&quot;]\"><img class=\"mw-file-element\" src=\"https://maps.wikimedia.org/img/osm-intl,16,a,a,250x200.png?lang=en&amp;domain=en.wikipedia.org&amp;title=Eiffel_Tower\" width=\"250\" height=\"200\"></a>";
    const MAPLINK: &str = "<p>Near <a class=\"mw-kartographer-maplink mw-kartographer-autostyled\" data-mw-kartographer=\"maplink\" data-style=\"osm-intl\" href=\"/wiki/Special:Map/13/-33.86/151.21/en\" data-zoom=\"13\" data-lat=\"-33.86\" data-lon=\"151.21\">33°52′S 151°13′E</a> and <a href=\"./Sydney\">Sydney</a>.</p>";

    #[test]
    fn interactive_maps_become_a_labelled_note_and_nothing_is_loaded() {
        let images = PackImageSources {
            base: READER_IMAGE_BASE.into(),
            by_file: HashMap::new(),
            stills: HashSet::new(),
        };
        for clean in [
            sanitize_pack_article_html(&format!("{TOKYO_MAP}{EIFFEL_MAP}{MAPLINK}"), &images),
            sanitize_online_article_html(&format!("{TOKYO_MAP}{EIFFEL_MAP}{MAPLINK}")),
        ] {
            assert!(!clean.contains("maps.wikimedia.org"), "{clean}");
            assert!(!clean.contains("<img"), "{clean}");
            assert!(!clean.contains("Special:Map"), "{clean}");
            assert!(
                clean.contains(
                    "<span class=\"foundation-map-note\">Map (online only) · 35.68° N, 139.41° E · \
                     <a href=\"https://www.openstreetmap.org/?mlat=35.68&amp;mlon=139.41#map=8/35.68/139.41\" \
                     rel=\"noopener noreferrer nofollow\">View on OpenStreetMap</a></span>\
                     <figcaption>Map of Tokyo</figcaption>"
                ),
                "{clean}"
            );
            // A map centred on its shapes has no stated centre to show.
            assert!(
                clean.contains("<span class=\"foundation-map-note\">Map (online only)</span>"),
                "{clean}"
            );
            // An inline map link keeps its text; ordinary links are kept.
            assert!(
                clean.contains("<p>Near 33°52′S 151°13′E and <a href="),
                "{clean}"
            );
            assert_eq!(clean.matches(MAP_NOTE).count(), 2, "{clean}");
        }
        // Out-of-range or non-numeric coordinates are never shown or linked.
        let odd = TOKYO_MAP
            .replace("data-lat=\"35.68\"", "data-lat=\"95\"")
            .replace("data-lon=\"139.41\"", "data-lon=\"javascript:x\"");
        let clean = sanitize_online_article_html(&odd);
        assert!(clean.contains("Map (online only)</span>"), "{clean}");
        assert!(!clean.contains("openstreetmap"), "{clean}");
        assert_eq!(trimmed(-0.5), "-0.5");
        assert_eq!(trimmed(48.858222), "48.85822");
    }

    #[test]
    fn a_pack_shows_its_drawn_maps_in_place_of_the_stored_notes() {
        // What a pack stores: the online sanitiser's output, maps as notes
        // in document order. A note the article itself wrote is renamed.
        let forged = "<p><span class=\"foundation-map-note\">not a map</span></p>";
        let stored = sanitize_online_article_html(&format!("{forged}{TOKYO_MAP}{EIFFEL_MAP}"));
        assert_eq!(
            stored.matches("class=\"foundation-map-note\"").count(),
            2,
            "{stored}"
        );
        assert!(stored.contains("foundation-map-note-text"), "{stored}");

        let digest = "cd".repeat(32);
        let images = PackImageSources {
            base: READER_IMAGE_BASE.into(),
            // Only the first map was drawn.
            by_file: HashMap::from([(map_image_key(0), digest.clone())]),
            stills: HashSet::new(),
        };
        let shown = sanitize_pack_article_html(&stored, &images);
        assert!(
            shown.contains(&format!(
                "<img src=\"{READER_IMAGE_BASE}{digest}\" alt=\"Map\">\
                 <span class=\"foundation-map-credit\">{MAP_CREDIT}</span>\
                 <figcaption>Map of Tokyo</figcaption>"
            )),
            "{shown}"
        );
        // The second map has no drawing: its note stays.
        assert_eq!(shown.matches(MAP_NOTE).count(), 1, "{shown}");
        assert!(shown.contains("not a map"), "{shown}");
        // Without drawn maps, or online, nothing is loaded for a map.
        for html in [
            sanitize_pack_article_html(
                &stored,
                &PackImageSources {
                    base: READER_IMAGE_BASE.into(),
                    by_file: HashMap::new(),
                    stills: HashSet::new(),
                },
            ),
            sanitize_online_article_html(&stored),
        ] {
            assert!(!html.contains("<img"), "{html}");
            assert!(!html.contains("foundation.invalid/map"), "{html}");
        }
        // A forged placeholder source never loads anything unmapped.
        let forged_source = "<img src=\"https://foundation.invalid/map/7\" alt=\"x\">";
        assert!(!sanitize_pack_article_html(forged_source, &images).contains("src="));
    }

    #[test]
    fn an_animation_packed_as_a_still_is_labelled_where_it_is_shown() {
        let still = "aa".repeat(32);
        let whole = "bb".repeat(32);
        let html = concat!(
            "<figure><img src=\"//upload.wikimedia.org/wikipedia/commons/5/52/Triple.gif\" ",
            "alt=\"phase a > b\"><figcaption>Triple point</figcaption></figure>",
            "<p>Text <img src=\"//upload.wikimedia.org/wikipedia/commons/9/94/Cycle.png\" alt=\"c\"></p>",
            "<p><img src=\"//upload.wikimedia.org/wikipedia/commons/0/00/Missing.gif\" alt=\"m\"></p>",
        );
        for base in ["packimage://localhost/", READER_IMAGE_BASE] {
            let images = PackImageSources {
                base: base.into(),
                by_file: HashMap::from([
                    ("Triple.gif".to_owned(), still.clone()),
                    ("Cycle.png".to_owned(), whole.clone()),
                ]),
                stills: HashSet::from([still.clone()]),
            };
            let clean = sanitize_pack_article_html(html, &images);
            let note = format!("<span class=\"foundation-still-note\">{STILL_IMAGE_NOTE}</span>");
            assert_eq!(clean.matches(&note).count(), 1, "{clean}");
            assert!(
                clean.contains(&format!(
                    "<img src=\"{base}{still}\" alt=\"phase a &gt; b\">{note}<figcaption>"
                )),
                "{clean}"
            );
            assert!(
                clean.contains(&format!("<img src=\"{base}{whole}\" alt=\"c\"></p>")),
                "{clean}"
            );
        }
    }

    #[test]
    fn pack_images_load_only_from_the_local_url_by_digest() {
        let digest = "ab".repeat(32);
        let html = concat!(
            "<a href=\"./File:Pythagorean.svg\" class=\"mw-file-description\">",
            "<img src=\"//upload.wikimedia.org/wikipedia/commons/thumb/d/d2/Pythagorean.svg/250px-Pythagorean.svg.png\" ",
            "srcset=\"//upload.wikimedia.org/x.png 2x\" alt=\"Proof\" width=\"250\"></a>",
            "<img src=\"//upload.wikimedia.org/wikipedia/commons/a/ab/Other.png\" alt=\"not in pack\">",
            "<img src=\"https://evil.example/wikipedia/commons/d/d2/Pythagorean.svg\" alt=\"evil\">",
            "<img src=\"packimage://localhost/cdcdcd\" alt=\"forged\">",
            "<a href=\"packimage://localhost/abab\">forged link</a>",
            "<a href=\" PACKIMAGE://localhost/abab\">forged link 2</a>",
            "<a href=\"https://foundation.invalid/media/abab\">forged link 3</a>",
        );
        for base in ["packimage://localhost/", READER_IMAGE_BASE] {
            let images = PackImageSources {
                base: base.into(),
                by_file: HashMap::from([("Pythagorean.svg".to_owned(), digest.clone())]),
                stills: HashSet::new(),
            };
            let clean = sanitize_pack_article_html(html, &images);
            assert!(
                clean.contains(&format!(
                    "<img src=\"{base}{digest}\" alt=\"Proof\" width=\"250\">"
                )),
                "{clean}"
            );
            assert_eq!(clean.matches("<img src=").count(), 1, "{clean}");
            assert!(!clean.contains("srcset"), "{clean}");
            assert!(!clean.contains("upload.wikimedia.org"), "{clean}");
            assert!(!clean.contains("evil.example"), "{clean}");
            assert!(!clean.contains("cdcdcd"), "{clean}");
            let lower = clean.to_ascii_lowercase();
            assert!(!lower.contains("href=\"packimage"), "{clean}");
            assert!(
                !lower.contains("href=\"https://foundation.invalid/media"),
                "{clean}"
            );
            assert!(clean.contains("alt=\"not in pack\""), "alt text stays");
        }
        // Without pack images nothing loads.
        assert!(!sanitize_article_html(html).contains("src="));
    }

    #[test]
    fn online_html_keeps_wikipedia_structure_and_only_wikimedia_images() {
        let html = concat!(
            "<table class=\"infobox\" id=\"box\"><tr><td>",
            "<img src=\"//upload.wikimedia.org/a/b.png\" srcset=\"//evil.example/x 2x\" alt=\"Map\" width=\"220\">",
            "</td></tr></table>",
            "<p>See <a href=\"/wiki/Moon\" title=\"Moon\">Moon</a> and ",
            "<a class=\"external\" href=\"https://example.org/\">a site</a>.</p>",
            "<span class=\"mwe-math-element\"><span class=\"mwe-math-mathml-inline\" style=\"display:none\">",
            "<math display=\"block\" href=\"javascript:x()\"><semantics><msup><mi>x</mi><mn>2</mn></msup><annotation>x^2</annotation></semantics></math></span>",
            "<img src=\"https://wikimedia.org/api/rest_v1/media/math/render/svg/abc\" alt=\"x^2\" class=\"mwe-math-fallback-image-inline\"></span>",
            "<img src=\"//thumb.wikimedia.org/wikipedia/commons/thumb/x/40px-x.png\" alt=\"s\">",
            "<img src=\"https://evil.example/track.gif\" alt=\"t\">",
            "<img src=\"data:image/png;base64,AAAA\" alt=\"d\">",
            "<div id=\"foundation-cited\" onclick=\"x()\" style=\"color:red\">c</div>",
            "<script>bad()</script><style>p{}</style>",
        );
        let clean = sanitize_online_article_html(html);
        assert!(clean.contains("class=\"infobox\""), "{clean}");
        assert!(
            clean.contains("src=\"https://upload.wikimedia.org/a/b.png\""),
            "{clean}"
        );
        assert!(
            clean.contains("src=\"https://wikimedia.org/api/rest_v1/media/math/render/svg/abc\"")
        );
        assert!(clean.contains("href=\"/wiki/Moon\""), "{clean}");
        assert!(clean.contains("href=\"https://example.org/\""));
        for forbidden in [
            "srcset",
            "evil.example",
            "data:",
            "<annotation",
            "x^2</",
            "onclick",
            "javascript:",
            "style=",
            "<script",
            "bad()",
            "foundation-cited",
        ] {
            assert!(!clean.contains(forbidden), "{forbidden} in {clean}");
        }
        // Native MathML is kept for display (its TeX annotation is not).
        assert!(clean.contains("<math display=\"block\"><semantics><msup><mi>x</mi><mn>2</mn></msup></semantics></math>"), "{clean}");
        // Alt text of removed images remains for the reader.
        assert!(clean.contains("alt=\"t\""));
    }

    use super::*;
    use wiki_model::{
        ArticleKey, Revision, ARTICLE_SCHEMA_VERSION, MAX_DISPLAY_BYTES, MAX_DISPLAY_NESTING,
        MAX_DISPLAY_REFERENCES,
    };

    fn article(html: &str) -> Article {
        Article {
            schema_version: ARTICLE_SCHEMA_VERSION,
            key: ArticleKey {
                project: "enwiki".into(),
                page_id: 9,
            },
            revision: Revision {
                revision_id: 12,
                timestamp: "2026-10-09T00:00:00Z".into(),
                content_sha256: "a".repeat(64),
            },
            title: "Physics".into(),
            display_title: "Physics <i>&</i>".into(),
            language: "en".into(),
            namespace: 0,
            aliases: vec![],
            wikidata_id: None,
            lead: vec![],
            sections: vec![],
            references: vec![],
            links: vec![],
            media: vec![],
            rendered_html: html.into(),
            is_disambiguation: false,
        }
    }

    #[test]
    fn the_android_document_carries_the_attribution_after_the_article() {
        let mut art = article("<p>Body</p>");
        art.media.push(wiki_model::MediaAsset {
            source_url: "https://upload.wikimedia.org/wikipedia/commons/a/ab/Figure.svg".into(),
            mime_type: "image/svg+xml".into(),
            sha256: Some("b".repeat(64)),
            license: "CC0".into(),
            creator: "Someone <b>".into(),
            attribution: String::new(),
            is_av_preview: false,
            derivation: None,
        });
        for theme in [ReaderTheme::Light, ReaderTheme::Dark] {
            let doc = reader_document(&art, theme);
            let main_end = doc.find("</main>").unwrap();
            let section = doc
                .find("<section class=\"foundation-attribution\">")
                .unwrap();
            assert!(section > main_end, "{doc}");
            assert!(doc.contains("<strong>Figure.svg</strong>: Someone &lt;b&gt; · CC0"));
            assert!(doc.contains("title=Physics&amp;oldid=12"));
            assert!(doc.contains("affiliated with the Wikimedia Foundation"));
        }
    }

    #[test]
    fn scripts_handlers_styles_frames_and_script_urls_are_removed() {
        let hostile = concat!(
            "<p onclick=\"steal()\" style=\"position:fixed\">Text</p>",
            "<script>steal()</script><style>body{display:none}</style>",
            "<iframe src=\"https://evil.example/\"></iframe>",
            "<a href=\"javascript:steal()\">x</a><a href=\"data:text/html,hi\">y</a>",
            "<form action=\"https://evil.example/\"><input name=q></form>",
            "<svg><script>steal()</script></svg>"
        );
        let clean = sanitize_article_html(hostile);
        for banned in [
            "onclick",
            "style=",
            "<script",
            "steal()",
            "<style",
            "<iframe",
            "javascript:",
            "data:",
            "<form",
            "<input",
            "<svg",
        ] {
            assert!(!clean.contains(banned), "{banned} survived: {clean}");
        }
        assert!(clean.contains("Text"));
    }

    #[test]
    fn article_links_are_rewritten_into_the_app_origin() {
        let clean = sanitize_article_html(concat!(
            "<a href=\"./Isaac_Newton\">Newton</a>",
            "<a href=\"/wiki/General_relativity#History\">GR</a>",
            "<a href=\"./AC/DC\">band</a>",
            "<a href=\"#cite_note-1\">[1]</a>",
            "<a href=\"https://example.org/x\">ext</a>"
        ));
        assert!(clean.contains("href=\"https://foundation.invalid/wiki/Isaac_Newton\""));
        assert!(
            clean.contains("href=\"https://foundation.invalid/wiki/General_relativity#History\"")
        );
        assert!(clean.contains("href=\"https://foundation.invalid/wiki/AC/DC\""));
        assert!(clean.contains("href=\"#cite_note-1\""));
        assert!(clean.contains("href=\"https://example.org/x\""));
        assert!(clean.contains("rel=\"noopener noreferrer nofollow\""));
    }

    #[test]
    fn other_relative_urls_are_dropped_so_nothing_resolves_against_the_origin() {
        let clean = sanitize_article_html(concat!(
            "<a href=\"../secret\">a</a><a href=\"/w/index.php?title=X\">b</a>",
            "<a href=\"./A/../../B\">c</a><a href=\"./X?action=raw\">d</a>",
            "<a href=\"//evil.example/x\">e</a><img src=\"./img.png\" alt=\"pic\">"
        ));
        for banned in [
            "secret",
            "index.php",
            "../",
            "action=raw",
            "evil.example",
            "img.png",
        ] {
            assert!(!clean.contains(banned), "{banned} survived: {clean}");
        }
    }

    #[test]
    fn the_document_is_escaped_themed_and_locked_down() {
        let light = reader_document(
            &article("<p id=\"lead\" class=\"x\">Body</p>"),
            ReaderTheme::Light,
        );
        assert!(light.contains("<h1>Physics &lt;i&gt;&amp;&lt;/i&gt;</h1>"));
        assert!(light.contains("<title>Physics &lt;i&gt;&amp;&lt;/i&gt;</title>"));
        assert!(light.contains("default-src 'none'"));
        assert!(light.contains("<p id=\"lead\" class=\"x\">Body</p>"));
        assert!(light.contains("Revision 12"));
        assert!(!light.contains("class=\"dark\""));
        let dark = reader_document(&article("<p>Body</p>"), ReaderTheme::Dark);
        assert!(dark.contains("<html lang=\"en\" class=\"dark\">"));
        let mut odd = article("<p>Body</p>");
        odd.language = "en\"><script>".into();
        assert!(reader_document(&odd, ReaderTheme::Light).contains("<html lang=\"en\">"));
    }

    fn cited_article(html: &str) -> Article {
        use wiki_model::{Block, BlockContent, Section};
        let mut article = article(html);
        article.lead = vec![Block {
            ordinal: 0,
            content: BlockContent::Paragraph("Physics studies matter.".into()),
        }];
        article.sections = vec![Section {
            ordinal: 1,
            heading: "Gravitation".into(),
            blocks: vec![Block {
                ordinal: 0,
                content: BlockContent::Paragraph("Gravity bends\n spacetime & time.".into()),
            }],
            subsections: vec![],
        }];
        article
    }

    const GRAVITY_ID: &str = "wkb:enwiki:9:12:1:b0";

    #[test]
    fn a_cited_paragraph_is_highlighted_in_place() {
        let article = cited_article(concat!(
            "<p>Physics studies matter.</p><h2 id=\"Gravitation\">Gravitation</h2>",
            "<p class=\"x\" title=\"a>b\">Gravity bends <b>spacetime</b> &amp; time.",
            "<sup class=\"reference\"><a href=\"#cite_note-1\">[1]</a></sup></p><p>After.</p>"
        ));
        assert_eq!(article.key.block_id(12, &[1], 0), GRAVITY_ID);
        let (document, located) =
            reader_document_with_citation(&article, ReaderTheme::Light, GRAVITY_ID).unwrap();
        assert_eq!(located, CitedPassage::Located);
        assert_eq!(document.matches("id=\"foundation-cited\"").count(), 1);
        let marked = document
            .split("<div id=\"foundation-cited\" class=\"foundation-cited\">")
            .nth(1)
            .unwrap();
        let marked = &marked[..marked.find("</div>").unwrap()];
        assert!(marked.starts_with("<p class=\"x\""), "{marked}");
        assert!(marked.contains("Gravity bends") && marked.ends_with("</p>"));
        assert!(!marked.contains("After."));
        let lead = article.key.block_id(12, &[], 0);
        let (document, located) =
            reader_document_with_citation(&article, ReaderTheme::Dark, &lead).unwrap();
        assert_eq!(located, CitedPassage::Located);
        assert!(
            document.contains("class=\"foundation-cited\"><p>Physics studies matter.</p></div>")
        );
    }

    #[test]
    fn a_citation_of_another_revision_or_block_is_not_highlighted() {
        let article = cited_article("<p>Gravity bends spacetime &amp; time.</p>");
        let stale = article.key.block_id(11, &[1], 0);
        assert!(reader_document_with_citation(&article, ReaderTheme::Light, &stale).is_none());
        assert!(reader_document_with_citation(
            &article,
            ReaderTheme::Light,
            "wkb:enwiki:9:12:4:b0"
        )
        .is_none());
    }

    #[test]
    fn an_unmatched_or_ambiguous_passage_is_shown_separately_never_guessed() {
        for html in [
            "<p>Something else entirely.</p>",
            "<p>Gravity bends spacetime &amp; time.</p><p>Gravity bends spacetime &amp; time.</p>",
        ] {
            let article = cited_article(html);
            let (document, located) =
                reader_document_with_citation(&article, ReaderTheme::Light, GRAVITY_ID).unwrap();
            assert_eq!(located, CitedPassage::ShownSeparately, "{html}");
            assert_eq!(document.matches("id=\"foundation-cited\"").count(), 1);
            assert!(document.contains("Cited passage (Gravitation)"));
            assert!(document.contains("<p>Gravity bends spacetime &amp; time.</p></div>"));
        }
    }

    #[test]
    fn article_markup_cannot_claim_the_reserved_anchor() {
        let clean = sanitize_article_html(
            "<p id=\"foundation-cited\">x</p><p id=\"foundation-other\">y</p><h2 id=\"History\">H</h2>",
        );
        assert!(!clean.contains("foundation-"), "{clean}");
        assert!(clean.contains("id=\"History\""));
    }
    /// STATE-003 A/V policy: audio and video are never played or fetched by
    /// either reader; their elements and media URLs are removed, the caption
    /// text stays.
    #[test]
    fn audio_and_video_never_stream_from_either_sanitizer() {
        let html = concat!(
            "<figure><video autoplay controls poster=\"https://upload.wikimedia.org/p.jpg\" ",
            "src=\"https://upload.wikimedia.org/v.webm\"><source src=\"https://upload.wikimedia.org/v.webm\">",
            "<track src=\"https://upload.wikimedia.org/v.vtt\"></video><figcaption>Launch film</figcaption></figure>",
            "<audio autoplay src=\"https://upload.wikimedia.org/a.ogg\"></audio>",
            "<object data=\"https://upload.wikimedia.org/x.swf\"></object><embed src=\"https://upload.wikimedia.org/e\">",
            "<iframe src=\"https://commons.wikimedia.org/embed\"></iframe><p>Text</p>",
        );
        for sanitized in [
            sanitize_article_html(html),
            sanitize_online_article_html(html),
        ] {
            for banned in [
                "<video", "<audio", "<source", "<track", "<object", "<embed", "<iframe",
                "autoplay", ".webm", ".ogg", ".vtt", ".swf", "/embed",
            ] {
                assert!(
                    !sanitized.contains(banned),
                    "{banned} survived: {sanitized}"
                );
            }
            assert!(sanitized.contains("Launch film"));
            assert!(sanitized.contains("<p>Text</p>"));
        }
    }

    /// MOBPACK-003: a player naming its file becomes a still and a note.
    /// Online the still loads from Wikimedia; from a pack it is the pack's
    /// own copy; a stored (already sanitised) article reads the same.
    #[test]
    fn audio_and_video_players_become_local_previews_with_a_play_online_link() {
        let html = concat!(
            "<figure><span><video poster=\"//thumb.wikimedia.org/wikipedia/commons/thumb/a/ab/Film.webm/250px-Film.webm.jpg?utm_source=x\" ",
            "controls=\"\" autoplay data-durationhint=\"95\" data-mwtitle=\"Film.webm\">",
            "<source src=\"//upload.wikimedia.org/wikipedia/commons/transcoded/a/ab/Film.webm/Film.webm.240p.vp9.webm\"></video></span>",
            "<figcaption>A film</figcaption></figure>",
            "<audio data-durationhint=\"88\" data-mwtitle=\"Speech.ogg\"><source src=\"//upload.wikimedia.org/a.mp3\"></audio>",
        );
        let note = "<span class=\"foundation-av-note\">Video · 1:35 · <a href=\"https://commons.wikimedia.org/wiki/File:Film.webm\" rel=\"noopener noreferrer nofollow\">Play online (needs Internet)</a></span>";
        let stored = sanitize_online_article_html(html);
        assert!(stored.contains(&format!(
            "<img src=\"https://thumb.wikimedia.org/wikipedia/commons/thumb/a/ab/Film.webm/250px-Film.webm.jpg\" alt=\"Video still\">{note}"
        )), "{stored}");
        assert!(stored.contains(
            ">Audio · 1:28 · <a href=\"https://commons.wikimedia.org/wiki/File:Speech.ogg\""
        ));
        assert!(stored.contains("<figcaption>A film</figcaption>"));

        let images = PackImageSources {
            base: READER_IMAGE_BASE.into(),
            by_file: HashMap::from([("Film.webm".to_owned(), "cd".repeat(32))]),
            stills: HashSet::new(),
        };
        for source in [html, stored.as_str()] {
            let pack = sanitize_pack_article_html(source, &images);
            assert!(
                pack.contains(&format!(
                    "<img src=\"{READER_IMAGE_BASE}{}\" alt=\"Video still\">{note}",
                    "cd".repeat(32)
                )),
                "{pack}"
            );
            for banned in [
                "<video",
                "<audio",
                "<source",
                "autoplay",
                "transcoded",
                ".mp3",
                "thumb.wikimedia.org",
            ] {
                assert!(!pack.contains(banned), "{banned} survived: {pack}");
            }
        }
        // Without images (or a pack that holds no still) nothing is loaded.
        let bare = sanitize_article_html(html);
        assert!(bare.contains("<img alt=\"Video still\">"), "{bare}");
        assert!(bare.contains(note));
    }

    /// TRUST-003: HTML beyond the display limits is replaced by a notice at
    /// once, in every sanitiser; HTML just within them is shown as usual.
    #[test]
    fn html_beyond_the_display_limits_gets_a_notice_quickly() {
        let started = std::time::Instant::now();
        let notice = format!("<p class=\"foundation-too-complex\">{TOO_COMPLEX_NOTE}</p>");
        let images = PackImageSources {
            base: READER_IMAGE_BASE.into(),
            by_file: HashMap::new(),
            stills: HashSet::new(),
        };
        for hostile in [
            format!("{}x{}", "<div>".repeat(200_000), "</div>".repeat(200_000)),
            "<div>".repeat(600),
            format!("<p>{}</p>", "&amp;".repeat(MAX_DISPLAY_REFERENCES + 1)),
            format!("<math>{}</math>", "<mrow>".repeat(600)),
            "a".repeat(MAX_DISPLAY_BYTES + 1),
        ] {
            assert_eq!(sanitize_article_html(&hostile), notice);
            assert_eq!(sanitize_online_article_html(&hostile), notice);
            assert_eq!(sanitize_pack_article_html(&hostile, &images), notice);
        }
        // Each refusal is a linear scan, not a parse.
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "{:?}",
            started.elapsed()
        );

        // Within the limits: shown as usual. Void elements, self-closed
        // tags, comments and quoted `<` or `>` do not count as nesting.
        let deep = format!(
            "{}x{}",
            "<div>".repeat(MAX_DISPLAY_NESTING),
            "</div>".repeat(MAX_DISPLAY_NESTING)
        );
        let flat = format!(
            "<div>{}<!-- {} --><span title=\"<div><div>\">ok</span></div>",
            "<br><img alt=\"a\"><hr/>".repeat(10_000),
            "<div>".repeat(1_000)
        );
        for fine in [deep, flat, "&amp;".repeat(1_000)] {
            assert_ne!(sanitize_article_html(&fine), notice);
        }
    }

    /// SRC-003: pack articles keep their formulas (MathML), never scripts
    /// or links inside them, and never TeX annotations.
    #[test]
    fn pack_articles_keep_mathml_formulas() {
        let html = concat!(
            "<p>Then <math class=\"mwe-math-element\" display=\"inline\"><semantics><mrow>",
            "<msup><mi>a</mi><mn>2</mn></msup><mo stretchy=\"false\">+</mo>",
            "<msup><mi href=\"javascript:x()\" onclick=\"x()\">b</mi><mn>2</mn></msup>",
            "</mrow><annotation encoding=\"application/x-tex\">a^2+b^2</annotation>",
            "</semantics></math>.</p>"
        );
        let sanitized = sanitize_article_html(html);
        assert!(
            sanitized.contains("<msup><mi>a</mi><mn>2</mn></msup>"),
            "{sanitized}"
        );
        assert!(sanitized.contains("<mo stretchy=\"false\">+</mo>"));
        assert!(!sanitized.contains("annotation") && !sanitized.contains("a^2+b^2"));
        assert!(!sanitized.contains("javascript") && !sanitized.contains("onclick"));
    }
}
