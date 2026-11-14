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
:root{--bg:#fff;--fg:#202122;--muted:#54595d;--rule:#a2a9b1;--link:#3366cc;--box:#f8f9fa}\
:root.dark{--bg:#101418;--fg:#eaecf0;--muted:#a2a9b1;--rule:#54595d;--link:#88a3e8;--box:#1b1f24}\
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
sup{line-height:1}";

/// A complete, script-free document for one verified article revision.
pub fn reader_document(article: &Article, theme: ReaderTheme) -> String {
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
        body = sanitize_article_html(&article.rendered_html),
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
}
