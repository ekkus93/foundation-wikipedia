//! The attribution section of an offline article page (SEC-002).
//!
//! Wikipedia text is shared under CC BY-SA 4.0, which requires credit,
//! a licence link and a note of changes, and images carry their own
//! creators and licences. The pack holds all of that, so the section is
//! built from the stored article alone and works with no Internet. Every
//! string comes from untrusted metadata and is escaped; every link is a
//! plain `https` URL built here or checked here.

use wiki_model::{Article, MediaAsset, MediaDerivation};

use crate::reader_document::escape_text;

/// Licence of Wikipedia article text. `Article` records no licence of its
/// own; the pack manifest states it for the whole pack.
pub const ARTICLE_LICENSE: &str = "CC BY-SA 4.0";
const ARTICLE_LICENSE_URL: &str = "https://creativecommons.org/licenses/by-sa/4.0/";

/// Shown at the end of every offline article.
pub const NON_AFFILIATION: &str = "This app is not made by or affiliated with the Wikimedia \
Foundation. Wikipedia is a trademark of the Wikimedia Foundation.";

/// What the pack did to an image, in plain words.
pub fn derivation_note(derivation: MediaDerivation) -> &'static str {
    match derivation {
        MediaDerivation::LosslessWebp => "Converted to WebP without loss of quality.",
        MediaDerivation::FirstFrameStill => {
            "First frame of an animation; the animation is online only."
        }
        MediaDerivation::SanitizedSvg => {
            "Vector image with scripts and editor data removed; the drawing is unchanged."
        }
        MediaDerivation::RenderedMap => "Map drawn by this pack from open map data.",
    }
}

fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.replace(' ', "_").bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~(),!*:".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn wiki_code(code: &str) -> Option<&str> {
    (!code.is_empty()
        && code.len() <= 32
        && code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
    .then_some(code)
}

/// The Commons or Wikipedia page describing the file behind an
/// `upload.wikimedia.org` URL.
fn file_page(source_url: &str) -> Option<String> {
    let name = crate::media_download::media_file_name(source_url)?;
    let url = url::Url::parse(source_url).ok()?;
    let wiki = url.path_segments()?.nth(1)?;
    let host = if wiki == "commons" {
        "commons.wikimedia.org".to_owned()
    } else {
        format!("{}.wikipedia.org", wiki_code(wiki)?)
    };
    Some(format!(
        "https://{host}/wiki/File:{}",
        percent_encode(&name)
    ))
}

/// The link shown for an asset's source: its file page, or the stored
/// URL itself when it is a plain `https` link (a drawn map's place).
fn source_link(asset: &MediaAsset) -> Option<String> {
    if let Some(page) = file_page(&asset.source_url) {
        return Some(page);
    }
    let url = url::Url::parse(&asset.source_url).ok()?;
    (url.scheme() == "https" && url.host_str().is_some()).then(|| url.to_string())
}

fn link(url: &str, text: &str) -> String {
    format!(
        "<a href=\"{}\" rel=\"noopener noreferrer nofollow\">{}</a>",
        escape_text(url),
        escape_text(text)
    )
}

fn credit(asset: &MediaAsset) -> String {
    let creator = asset.creator.trim();
    let license = asset.license.trim();
    let mut text = String::from("<li>");
    match crate::media_download::media_file_name(&asset.source_url) {
        Some(name) => text.push_str(&format!("<strong>{}</strong>: ", escape_text(&name))),
        None if asset.derivation == Some(MediaDerivation::RenderedMap) => {
            text.push_str("<strong>Map</strong>: ");
        }
        None => {}
    }
    text.push_str(&format!(
        "{} · {}",
        if creator.is_empty() {
            "Creator not stated".to_owned()
        } else {
            escape_text(creator)
        },
        if license.is_empty() {
            "licence not stated".to_owned()
        } else {
            escape_text(license)
        },
    ));
    if let Some(url) = source_link(asset) {
        text.push_str(&format!(" · {}", link(&url, "Source")));
    }
    if let Some(derivation) = asset.derivation {
        text.push_str(&format!(
            " <span class=\"foundation-attribution-note\">{}</span>",
            derivation_note(derivation)
        ));
    }
    text.push_str("</li>");
    text
}

/// The attribution section for `article`, as a block of HTML for the end
/// of the page: licence, revision and history links, one credit per
/// image the pack holds, and the non-affiliation line.
pub fn attribution_section(article: &Article) -> String {
    let mut out = String::from("<section class=\"foundation-attribution\"><h2>Attribution</h2>");
    let title = percent_encode(&article.title);
    let wikipedia = wiki_code(&article.language).filter(|_| !article.title.is_empty());
    out.push_str(&format!(
        "<p>Text of this article is from Wikipedia and available under {}.",
        link(ARTICLE_LICENSE_URL, ARTICLE_LICENSE)
    ));
    if let Some(code) = wikipedia {
        let base = format!("https://{code}.wikipedia.org/w/index.php?title={title}");
        out.push_str(&format!(
            " Revision {} · {} · {}.",
            article.revision.revision_id,
            link(
                &format!("{base}&oldid={}", article.revision.revision_id),
                "View this revision"
            ),
            link(
                &format!("{base}&action=history"),
                "Page history and contributors"
            ),
        ));
    }
    out.push_str(" This offline copy may differ in layout from the original.</p>");
    if !article.media.is_empty() {
        out.push_str("<h3>Media credits</h3><ul>");
        for asset in &article.media {
            out.push_str(&credit(asset));
        }
        out.push_str("</ul>");
    }
    out.push_str(&format!(
        "<p class=\"foundation-attribution-note\">{}</p></section>",
        escape_text(NON_AFFILIATION)
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiki_model::{ArticleKey, Revision};

    fn asset(url: &str, creator: &str, derivation: Option<MediaDerivation>) -> MediaAsset {
        MediaAsset {
            source_url: url.into(),
            mime_type: "image/webp".into(),
            sha256: Some("a".repeat(64)),
            license: "CC BY-SA 4.0".into(),
            creator: creator.into(),
            attribution: String::new(),
            is_av_preview: false,
            derivation,
        }
    }

    fn article(media: Vec<MediaAsset>) -> Article {
        Article {
            schema_version: 1,
            key: ArticleKey {
                project: "enwiki".into(),
                page_id: 7,
            },
            revision: Revision {
                revision_id: 42,
                timestamp: "2026-01-01T00:00:00Z".into(),
                content_sha256: "0".repeat(64),
            },
            title: "Pythagorean theorem".into(),
            display_title: "Pythagorean theorem".into(),
            language: "en".into(),
            namespace: 0,
            aliases: vec![],
            wikidata_id: None,
            lead: vec![],
            sections: vec![],
            references: vec![],
            links: vec![],
            media,
            rendered_html: String::new(),
            is_disambiguation: false,
        }
    }

    const COMMONS: &str =
        "https://upload.wikimedia.org/wikipedia/commons/a/ab/Hypotenuse%20%26x.svg";

    #[test]
    fn licence_revision_history_and_non_affiliation_are_shown() {
        let html = attribution_section(&article(vec![]));
        assert!(html.contains("https://creativecommons.org/licenses/by-sa/4.0/"));
        assert!(html.contains(
            "https://en.wikipedia.org/w/index.php?title=Pythagorean_theorem&amp;oldid=42"
        ));
        assert!(html.contains("title=Pythagorean_theorem&amp;action=history"));
        assert!(html.contains("not made by or affiliated with the Wikimedia Foundation"));
        assert!(!html.contains("Media credits"));
    }

    #[test]
    fn every_image_is_credited_with_its_file_page_and_derivation() {
        let media = vec![
            asset(COMMONS, "Émilie", Some(MediaDerivation::SanitizedSvg)),
            asset(
                "https://upload.wikimedia.org/wikipedia/en/1/12/Local.png",
                "B",
                Some(MediaDerivation::LosslessWebp),
            ),
            asset(
                "https://upload.wikimedia.org/wikipedia/commons/2/2a/Anim.gif",
                "C",
                Some(MediaDerivation::FirstFrameStill),
            ),
            asset(
                "https://upload.wikimedia.org/wikipedia/commons/3/3a/Plain.jpg",
                "D",
                None,
            ),
        ];
        let html = attribution_section(&article(media));
        assert_eq!(html.matches("<li>").count(), 4, "{html}");
        assert!(html.contains("https://commons.wikimedia.org/wiki/File:Hypotenuse_%26x.svg"));
        assert!(html.contains("https://en.wikipedia.org/wiki/File:Local.png"));
        assert!(html.contains("Émilie"));
        for derivation in [
            MediaDerivation::SanitizedSvg,
            MediaDerivation::LosslessWebp,
            MediaDerivation::FirstFrameStill,
        ] {
            assert!(html.contains(derivation_note(derivation)), "{html}");
        }
        assert_eq!(html.matches("Converted to WebP").count(), 1);
    }

    #[test]
    fn drawn_maps_are_credited_with_their_own_creator_and_licence() {
        let mut map = asset(
            "https://www.openstreetmap.org/?mlat=1.00000&mlon=2.00000#map=7/1.00000/2.00000",
            "OpenStreetMap contributors",
            Some(MediaDerivation::RenderedMap),
        );
        map.license = "ODbL 1.0".into();
        let html = attribution_section(&article(vec![map]));
        assert!(html.contains("<strong>Map</strong>: OpenStreetMap contributors · ODbL 1.0"));
        assert!(html.contains("href=\"https://www.openstreetmap.org/?mlat=1.00000"));
        assert!(html.contains("Map drawn by this pack from open map data."));
    }

    #[test]
    fn untrusted_metadata_is_escaped_and_bad_links_are_dropped() {
        let mut hostile = asset(
            "javascript:alert(1)",
            "<script>alert(1)</script>\"><img src=x onerror=y>",
            None,
        );
        hostile.license = "<b>free</b>".into();
        let mut art = article(vec![hostile, asset("", "", None)]);
        art.title = "A\"><script>".into();
        art.language = "en\"><x".into();
        let html = attribution_section(&art);
        assert!(!html.contains("<script"), "{html}");
        assert!(!html.contains("<img"), "{html}");
        assert!(!html.contains("<b>"), "{html}");
        assert!(!html.contains("javascript:"), "{html}");
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("Creator not stated"));
        assert!(
            !html.contains("wikipedia.org/w/index.php"),
            "bad language: no links"
        );
        assert_eq!(html.matches("<a ").count(), 1, "only the licence link");
    }
}
