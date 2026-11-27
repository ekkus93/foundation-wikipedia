//! SRC-003: real licence, creator and attribution for the images an article
//! requires, from the public Action API (`prop=imageinfo` with
//! `extmetadata`). The result is one `MediaAsset` per requested file, with
//! `sha256: None`: the image is required offline but its bytes are not
//! installed yet (PACK-003 downloads and verifies them).
//!
//! The response is untrusted. Every field is checked, HTML in Artist/Credit
//! is reduced to plain text, and a file without a stated licence fails
//! closed. Nothing is invented: when no creator is stated, the creator says so.

use serde_json::Value;
use wiki_model::MediaAsset;

const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const MAX_FIELD_CHARS: usize = 2_000;
const NO_CREATOR: &str = "Not stated on the file description page";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaMetadataError {
    ResponseTooLarge,
    InvalidResponse,
    ApiError,
    /// The file does not exist (no image information was returned).
    MissingFile(String),
    /// The file page states no licence.
    MissingLicense(String),
    /// The file URL is not an official Wikimedia upload URL.
    UnexpectedUrl(String),
}

/// Plain text of a small HTML fragment: tags dropped, common entities
/// decoded, whitespace collapsed, length bounded.
fn plain_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                text.push(' ');
            }
            '>' if in_tag => in_tag = false,
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    let decoded = text
        .replace("&nbsp;", " ")
        .replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    let collapsed = decoded
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    // Punctuation glued to removed tags: "Dnu72 ( talk )" → "Dnu72 (talk)".
    let tidy = collapsed
        .replace(" ,", ",")
        .replace(" .", ".")
        .replace(" :", ":")
        .replace("( ", "(")
        .replace(" )", ")");
    tidy.chars().take(MAX_FIELD_CHARS).collect()
}

fn field(meta: &Value, name: &str) -> Option<String> {
    meta.get(name)
        .and_then(|entry| entry.get("value"))
        .and_then(Value::as_str)
        .map(plain_text)
        .filter(|text| !text.is_empty())
}

/// An official upload URL without tracking parameters.
pub(crate) fn upload_url(raw: &str) -> Option<String> {
    let mut url = url::Url::parse(raw).ok()?;
    let official = url.scheme() == "https"
        && url.host_str() == Some("upload.wikimedia.org")
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none();
    if !official {
        return None;
    }
    url.set_query(None);
    url.set_fragment(None);
    Some(url.into())
}

/// The type of a file an article shows: an image, or a video whose still
/// is what a pack keeps (MOBPACK-003; Ogg video is `application/ogg`).
fn mime(value: &str) -> bool {
    let Some((kind, subtype)) = value.split_once('/') else {
        return false;
    };
    (matches!(kind, "image" | "video") || value == "application/ogg")
        && value.len() <= 64
        && !subtype.is_empty()
        && subtype
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'+'))
}

/// The `imageinfo` object of each requested file, in `files` order. `files`
/// are the `File:` titles that were requested; titles the API normalized
/// (e.g. underscores) map back to the request.
pub(crate) fn image_infos(body: &str, files: &[String]) -> Result<Vec<Value>, MediaMetadataError> {
    if body.len() > MAX_RESPONSE_BYTES {
        return Err(MediaMetadataError::ResponseTooLarge);
    }
    let value: Value =
        crate::strict_json::parse(body).map_err(|_| MediaMetadataError::InvalidResponse)?;
    if value.get("error").is_some() {
        return Err(MediaMetadataError::ApiError);
    }
    // With a single title the API offers to continue into the file's older
    // versions (`iistart`); the first entry is the current file, which is
    // all that is used. Any other continuation means missing pages.
    if let Some(next) = value.get("continue") {
        let only_older_versions = next.as_object().is_some_and(|fields| {
            fields.contains_key("iistart")
                && fields
                    .keys()
                    .all(|key| key == "iistart" || key == "continue")
        });
        if !only_older_versions {
            return Err(MediaMetadataError::InvalidResponse);
        }
    }
    let pages = value
        .get("query")
        .and_then(|query| query.get("pages"))
        .and_then(Value::as_array)
        .ok_or(MediaMetadataError::InvalidResponse)?;
    let normalized: Vec<(String, String)> = value
        .get("query")
        .and_then(|query| query.get("normalized"))
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    Some((
                        entry.get("from")?.as_str()?.to_owned(),
                        entry.get("to")?.as_str()?.to_owned(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    files
        .iter()
        .map(|file| {
            let title = normalized
                .iter()
                .find(|(from, _)| from == file)
                .map_or(file.as_str(), |(_, to)| to.as_str());
            pages
                .iter()
                .find(|page| page.get("title").and_then(Value::as_str) == Some(title))
                .and_then(|page| page.get("imageinfo"))
                .and_then(Value::as_array)
                .and_then(|infos| infos.first())
                .cloned()
                .ok_or_else(|| MediaMetadataError::MissingFile(file.clone()))
        })
        .collect()
}

/// One `MediaAsset` per `files` entry, in the same order. `files` are the
/// `File:` titles that were requested (as `rendered_article` names them).
pub fn parse_image_metadata(
    body: &str,
    files: &[String],
) -> Result<Vec<MediaAsset>, MediaMetadataError> {
    let infos = image_infos(body, files)?;
    files
        .iter()
        .zip(&infos)
        .map(|(file, info)| {
            let source_url = info
                .get("url")
                .and_then(Value::as_str)
                .and_then(upload_url)
                .ok_or_else(|| MediaMetadataError::UnexpectedUrl(file.clone()))?;
            let mime_type = info
                .get("mime")
                .and_then(Value::as_str)
                .filter(|value| mime(value))
                .ok_or(MediaMetadataError::InvalidResponse)?
                .to_owned();
            let empty = Value::Null;
            let meta = info.get("extmetadata").unwrap_or(&empty);
            let license = field(meta, "LicenseShortName")
                .ok_or_else(|| MediaMetadataError::MissingLicense(file.clone()))?;
            let creator = field(meta, "Artist")
                .or_else(|| field(meta, "Credit"))
                .unwrap_or_else(|| NO_CREATOR.to_owned());
            let description = info
                .get("descriptionurl")
                .and_then(Value::as_str)
                .filter(|url| url.starts_with("https://commons.wikimedia.org/wiki/File:"))
                .map(str::to_owned);
            let attribution = field(meta, "Attribution").unwrap_or_else(|| {
                let mut text = format!("{creator}, {license}, via Wikimedia Commons");
                if let Some(url) = &description {
                    text.push_str(&format!(" ({url})"));
                }
                text
            });
            Ok(MediaAsset {
                source_url,
                mime_type,
                sha256: None,
                license,
                creator,
                attribution,
                is_av_preview: false,
                derivation: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HYPOTENUSE_IMAGES: &str =
        include_str!("../../../fixtures/wikimedia/enwiki/hypotenuse-1383416967.images.json");

    fn files() -> Vec<String> {
        [
            "File:Hypotenuse.svg",
            "File:Pythagorean.svg",
            "File:Trigono sine en2.svg",
        ]
        .map(str::to_owned)
        .to_vec()
    }

    #[test]
    fn real_commons_metadata_becomes_required_not_installed_media() {
        let media = parse_image_metadata(HYPOTENUSE_IMAGES, &files()).unwrap();
        assert_eq!(media.len(), 3);
        // Same order as requested, not the response order.
        assert!(media[0].source_url.ends_with("/Hypotenuse.svg"));
        assert!(media[1].source_url.ends_with("/Pythagorean.svg"));
        assert_eq!(
            media[2].source_url,
            "https://upload.wikimedia.org/wikipedia/commons/e/ed/Trigono_sine_en2.svg"
        );
        for asset in &media {
            assert_eq!(asset.sha256, None, "bytes are not installed");
            assert_eq!(asset.mime_type, "image/svg+xml");
            assert!(!asset.license.is_empty());
            assert!(!asset.creator.contains('<'));
            assert!(asset.attribution.contains(&asset.license));
        }
        assert_eq!(media[1].license, "CC BY-SA 3.0");
        assert_eq!(media[1].creator, "en:User:Wapcaplet");
        assert!(media[2].creator.contains("Dnu72 (talk)") || media[2].creator.contains("Dnu72"));
        assert!(media[1].attribution.ends_with(
            "via Wikimedia Commons (https://commons.wikimedia.org/wiki/File:Pythagorean.svg)"
        ));
    }

    #[test]
    fn missing_files_licences_and_foreign_urls_fail_closed() {
        let body = |info: &str| {
            format!(
                r#"{{"batchcomplete":true,"query":{{"pages":[{{"ns":6,"title":"File:A.png"{info}}}]}}}}"#
            )
        };
        let files = vec!["File:A.png".to_owned()];
        assert_eq!(
            parse_image_metadata(&body(r#","missing":true"#), &files),
            Err(MediaMetadataError::MissingFile("File:A.png".into()))
        );
        let info = |url: &str, meta: &str| {
            format!(
                r#","imageinfo":[{{"url":"{url}","mime":"image/png","extmetadata":{{{meta}}}}}]"#
            )
        };
        let licence = r#""LicenseShortName":{"value":"Public domain"}"#;
        assert_eq!(
            parse_image_metadata(
                &body(&info("https://upload.wikimedia.org/a/A.png", "")),
                &files
            ),
            Err(MediaMetadataError::MissingLicense("File:A.png".into()))
        );
        assert_eq!(
            parse_image_metadata(&body(&info("https://evil.example/A.png", licence)), &files),
            Err(MediaMetadataError::UnexpectedUrl("File:A.png".into()))
        );
        // No creator stated: say so rather than invent one.
        let media = parse_image_metadata(
            &body(&info("https://upload.wikimedia.org/a/A.png", licence)),
            &files,
        )
        .unwrap();
        assert_eq!(media[0].creator, NO_CREATOR);
        assert_eq!(media[0].license, "Public domain");
        assert_eq!(
            parse_image_metadata(r#"{"error":{"code":"x"}}"#, &files),
            Err(MediaMetadataError::ApiError)
        );
        assert_eq!(
            parse_image_metadata(
                r#"{"batchcomplete":true,"continue":{},"query":{"pages":[]}}"#,
                &files
            ),
            Err(MediaMetadataError::InvalidResponse)
        );
        // A single-file request may offer older file versions: the current
        // version is used. Continuing to more pages is still refused.
        let page = body(&info("https://upload.wikimedia.org/a/A.png", licence));
        let older = page.replacen(
            '{',
            r#"{"continue":{"iistart":"2019-07-18T07:44:31Z","continue":"||"},"#,
            1,
        );
        assert_eq!(parse_image_metadata(&older, &files).unwrap().len(), 1);
        let more_pages = page.replacen(
            '{',
            r#"{"continue":{"iistart":"2019-07-18T07:44:31Z","iicontinue":"B.png","continue":"||"},"#,
            1,
        );
        assert_eq!(
            parse_image_metadata(&more_pages, &files),
            Err(MediaMetadataError::InvalidResponse)
        );
    }

    #[test]
    fn a_real_revision_assembles_with_its_real_image_metadata() {
        use crate::online_article::{parse_rendered_revision_with_source, OnlineRevision};
        use crate::rendered_article::{assemble_article, extract_rendered_article};
        let parse =
            include_str!("../../../fixtures/wikimedia/enwiki/hypotenuse-1383416967.parse.json");
        let pinned = OnlineRevision {
            key: wiki_model::ArticleKey {
                project: "enwiki".into(),
                page_id: 251366,
            },
            title: "Hypotenuse".into(),
            namespace: 0,
            revision_id: 1383416967,
            timestamp: "2026-11-02T15:48:37Z".into(),
        };
        let (rendered, raw) = parse_rendered_revision_with_source(parse, &pinned).unwrap();
        let extracted = extract_rendered_article(&raw).unwrap();
        let requested: Vec<String> = extracted
            .images
            .iter()
            .map(|i| i.file_title.clone())
            .collect();
        assert_eq!(requested, files());
        let media = parse_image_metadata(HYPOTENUSE_IMAGES, &requested).unwrap();
        let article = assemble_article(&pinned, "en", extracted, rendered.html, media).unwrap();
        article.validate().unwrap();
        assert!(article.media.iter().all(|asset| asset.sha256.is_none()));
    }

    #[test]
    fn the_image_metadata_request_is_a_canonical_transport_url() {
        let api = crate::online_requests::WikipediaRequests::new("enwiki").unwrap();
        let url = api.image_metadata(&files()).unwrap();
        assert!(api.accepts_transport_url(&url));
        assert!(!api.accepts_transport_url(&url.replace("iiprop=url", "iiprop=user")));
        assert!(!api.accepts_transport_url(&format!("{url}&iiprop=url")));
        assert!(api.image_metadata(&[]).is_err());
        assert!(api.image_metadata(&["Hypotenuse".to_owned()]).is_err());
        assert!(api.image_metadata(&["File:A|File:B".to_owned()]).is_err());
        let many: Vec<String> = (0..51).map(|n| format!("File:{n}.png")).collect();
        assert!(api.image_metadata(&many).is_err());
    }
}
