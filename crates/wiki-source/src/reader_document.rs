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
//! * image sources are removed until verified offline media exists.
//!
//! The document is wrapped with Wikipedia-like styling and a strict Content
//! Security Policy. `foundation.invalid` is a reserved name that never
//! resolves; the app also refuses every subresource request, so nothing
//! leaves the device. Offline media is served later through the same origin.

use std::borrow::Cow;
use std::collections::HashSet;

use ammonia::{Builder, UrlRelative};
use wiki_model::Article;

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

/// Sanitize untrusted article HTML for display.
pub fn sanitize_article_html(html: &str) -> String {
    let mut builder = Builder::default();
    builder
        .url_schemes(HashSet::from(["http", "https"]))
        .url_relative(UrlRelative::Custom(Box::new(rewrite_relative)))
        // Keep heading/citation anchors and Wikipedia's presentational classes;
        // with scripts and styles removed they cannot run or restyle the app.
        .add_generic_attributes(["id", "class"])
        // No verified offline media exists yet (PACK-003): drop image sources
        // so no image URL is ever requested; alt text remains.
        .rm_tag_attributes("img", ["src", "srcset"])
        .id_prefix(None)
        // The app's own anchors must not be claimed by article markup.
        .attribute_filter(|_, attribute, value| {
            if attribute == "id" && value.starts_with(RESERVED_ID_PREFIX) {
                None
            } else {
                Some(value.into())
            }
        })
        .link_rel(Some("noopener noreferrer nofollow"));
    builder.clean(html).to_string()
}

fn escape_text(text: &str) -> String {
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
.foundation-cited-note{color:var(--muted);font-size:.8em;margin:0}";

/// A complete, script-free document for one verified article revision.
pub fn reader_document(article: &Article, theme: ReaderTheme) -> String {
    wrap_document(
        article,
        theme,
        &sanitize_article_html(&article.rendered_html),
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
    let body = sanitize_article_html(&article.rendered_html);
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
<main>{body}</main></body></html>",
        scheme = match theme {
            ReaderTheme::Light => "light",
            ReaderTheme::Dark => "dark",
        },
        title = escape_text(&article.display_title),
        revision = article.revision.revision_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiki_model::{ArticleKey, Revision, ARTICLE_SCHEMA_VERSION};

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
}
