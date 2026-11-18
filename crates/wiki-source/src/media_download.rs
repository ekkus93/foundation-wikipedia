//! PACK-003: which bytes to download for each image an article requires,
//! and how to check them before they go into a pack.
//!
//! The `imageinfo` reply (requested with `size|sha1` and
//! [`OFFLINE_IMAGE_WIDTH`]) is untrusted. For each file:
//!
//! * a PNG, JPEG, GIF or WebP no wider than [`OFFLINE_IMAGE_WIDTH`] is
//!   downloaded as the original, and must match Wikimedia's stated size and
//!   SHA-1;
//! * an SVG up to [`MAX_SVG_SOURCE_BYTES`] is downloaded as the original,
//!   checked against Wikimedia's size and SHA-1, so the pack builder can keep
//!   it as a vector once sanitised (`wiki_store::pack_svg`). Its 1280 px
//!   rendering is the [`ImageDownload::fallback`] for an SVG the sanitiser
//!   refuses;
//! * anything else — a larger SVG, a wider raster, TIFF, PDF, a video
//!   still — is downloaded as Wikimedia's own rendering at that width.
//!   Wikimedia states no hash for a rendering, so only TLS to the fixed
//!   media hosts, the size cap and the file signature check it. Wikimedia's
//!   thumbnail service picks the format from `Accept` (it serves WebP behind
//!   a `.png` URL when WebP is accepted); WebP is preferred as the smallest,
//!   and the stored format is whatever the bytes' own signature says.
//!
//! An original must start with the signature of its stated format, and any
//! download must be PNG, JPEG, GIF or WebP, so a mislabelled or substituted
//! reply is refused.

use serde_json::Value;
use sha1::{Digest, Sha1};

use crate::online_requests::OFFLINE_IMAGE_WIDTH;
use crate::rendered_media::{image_infos, MediaMetadataError};

/// Largest image accepted into a pack.
pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
/// Largest SVG original downloaded to be kept as a vector.
pub const MAX_SVG_SOURCE_BYTES: u64 = 4 * 1024 * 1024;
const SVG: &str = "image/svg+xml";

/// Hosts Wikimedia serves file bytes from.
const MEDIA_HOSTS: [&str; 2] = ["upload.wikimedia.org", "thumb.wikimedia.org"];

/// Fetches image bytes. Implementations must accept only URLs for which
/// [`media_url_allowed`] holds, never follow redirects and keep TLS checks.
pub trait MediaFileTransport {
    type Error;
    /// `accept` is the HTTP `Accept` value, [`ImageDownload::accept`].
    fn get_media(
        &mut self,
        url: &str,
        accept: &str,
        max_bytes: usize,
    ) -> Result<Vec<u8>, Self::Error>;
}

/// `Accept` for a rendering: WebP first, any other pack format allowed.
pub const RENDERING_ACCEPT: &str = "image/webp, image/png;q=0.9, image/jpeg;q=0.9, image/gif;q=0.9";

/// One image to download for a pack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageDownload {
    /// `File:` title as requested.
    pub file_title: String,
    /// Canonical media URL (no query or fragment).
    pub url: String,
    /// For an original: its format, size and Wikimedia's SHA-1. `None`
    /// for a rendering, whose format is negotiated.
    pub expected: Option<Original>,
    /// For an SVG original: its rendering's URL, used if the SVG cannot be
    /// kept as a vector.
    pub fallback: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Original {
    pub mime: &'static str,
    pub sha1: String,
    pub size: u64,
}

impl ImageDownload {
    /// The `Accept` value to request this download with.
    pub fn accept(&self) -> &'static str {
        self.expected
            .as_ref()
            .map_or(RENDERING_ACCEPT, |original| original.mime)
    }

    /// The rendering to download instead of an SVG original.
    pub fn fallback_download(&self) -> Option<ImageDownload> {
        Some(ImageDownload {
            file_title: self.file_title.clone(),
            url: self.fallback.clone()?,
            expected: None,
            fallback: None,
        })
    }

    /// Whether this is an SVG original, to be sanitised before packing.
    pub fn is_svg_original(&self) -> bool {
        self.expected
            .as_ref()
            .is_some_and(|original| original.mime == SVG)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageDownloadError {
    Metadata(MediaMetadataError),
    /// The file has no usable original or rendering URL.
    NoUsableUrl(String),
    TooLarge,
    /// Bytes do not start with the claimed format's signature.
    WrongFormat,
    /// Size or SHA-1 differs from what Wikimedia states.
    DigestMismatch,
}

/// A canonical media URL: HTTPS on a Wikimedia media host, a
/// `/wikipedia/<wiki>/…` path of safe characters, nothing else.
pub fn media_url_allowed(candidate: &str) -> bool {
    let Ok(url) = url::Url::parse(candidate) else {
        return false;
    };
    let path = url.path();
    url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|host| MEDIA_HOSTS.contains(&host))
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.as_str() == candidate
        && path.len() <= 1024
        && path.starts_with("/wikipedia/")
        && !path.contains("/../")
        && !path.contains("//")
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/%-_.,()!~'".contains(&b))
}

/// The URL without tracking parameters, if it is an allowed media URL.
fn canonical_media_url(raw: &str) -> Option<String> {
    let mut url = url::Url::parse(raw).ok()?;
    url.set_query(None);
    url.set_fragment(None);
    let url = String::from(url);
    media_url_allowed(&url).then_some(url)
}

fn web_raster(mime: &str) -> Option<&'static str> {
    ["image/png", "image/jpeg", "image/gif", "image/webp"]
        .into_iter()
        .find(|known| *known == mime)
}

fn rendering_mime(url: &str) -> Option<&'static str> {
    let extension = url.rsplit_once('.')?.1.to_ascii_lowercase();
    match extension.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

fn hex_sha1(value: &str) -> Option<String> {
    (value.len() == 40 && value.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| value.to_ascii_lowercase())
}

/// What to download for each of `files`, in the same order.
pub fn parse_image_downloads(
    body: &str,
    files: &[String],
) -> Result<Vec<ImageDownload>, ImageDownloadError> {
    let infos = image_infos(body, files).map_err(ImageDownloadError::Metadata)?;
    files
        .iter()
        .zip(&infos)
        .map(|(file, info)| choose(file, info))
        .collect()
}

fn choose(file: &str, info: &Value) -> Result<ImageDownload, ImageDownloadError> {
    let unusable = || ImageDownloadError::NoUsableUrl(file.to_owned());
    let mime = info.get("mime").and_then(Value::as_str).unwrap_or("");
    let width = info.get("width").and_then(Value::as_u64).unwrap_or(0);
    if let Some(mime) = web_raster(mime) {
        if width > 0 && width <= u64::from(OFFLINE_IMAGE_WIDTH) {
            let url = info
                .get("url")
                .and_then(Value::as_str)
                .and_then(canonical_media_url)
                .ok_or_else(unusable)?;
            let sha1 = info
                .get("sha1")
                .and_then(Value::as_str)
                .and_then(hex_sha1)
                .ok_or_else(unusable)?;
            let size = info
                .get("size")
                .and_then(Value::as_u64)
                .filter(|size| *size > 0 && *size <= MAX_IMAGE_BYTES as u64)
                .ok_or(ImageDownloadError::TooLarge)?;
            return Ok(ImageDownload {
                file_title: file.to_owned(),
                url,
                expected: Some(Original { mime, sha1, size }),
                fallback: None,
            });
        }
    }
    let rendering = info
        .get("thumburl")
        .and_then(Value::as_str)
        .and_then(canonical_media_url)
        .ok_or_else(unusable)?;
    // A rendering URL names a raster format even though the reply may differ.
    rendering_mime(&rendering).ok_or_else(unusable)?;
    let size = info.get("size").and_then(Value::as_u64).unwrap_or(0);
    if mime == SVG && size > 0 && size <= MAX_SVG_SOURCE_BYTES {
        let original = info
            .get("url")
            .and_then(Value::as_str)
            .and_then(canonical_media_url)
            .filter(|url| url.to_ascii_lowercase().ends_with(".svg"));
        let sha1 = info.get("sha1").and_then(Value::as_str).and_then(hex_sha1);
        if let (Some(url), Some(sha1)) = (original, sha1) {
            return Ok(ImageDownload {
                file_title: file.to_owned(),
                url,
                expected: Some(Original {
                    mime: SVG,
                    sha1,
                    size,
                }),
                fallback: Some(rendering),
            });
        }
    }
    Ok(ImageDownload {
        file_title: file.to_owned(),
        url: rendering,
        expected: None,
        fallback: None,
    })
}

/// The pack format `bytes` start with, by file signature.
pub fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// Check downloaded bytes against what was asked for; returns their format.
pub fn verify_image(
    download: &ImageDownload,
    bytes: &[u8],
) -> Result<&'static str, ImageDownloadError> {
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(ImageDownloadError::TooLarge);
    }
    // An SVG has no signature; its hash identifies it, and the pack
    // builder's sanitiser decides whether it is kept.
    let mime = match &download.expected {
        Some(original) if original.mime == SVG => SVG,
        _ => sniff_image(bytes).ok_or(ImageDownloadError::WrongFormat)?,
    };
    if let Some(original) = &download.expected {
        if mime != original.mime {
            return Err(ImageDownloadError::WrongFormat);
        }
        let actual = hex(&Sha1::digest(bytes));
        if bytes.len() as u64 != original.size || actual != original.sha1 {
            return Err(ImageDownloadError::DigestMismatch);
        }
    }
    Ok(mime)
}

/// The file name a Wikimedia media URL serves, with underscores, e.g.
/// `Hypotenuse.svg` for both `…/commons/8/8a/Hypotenuse.svg` and
/// `…/commons/thumb/8/8a/Hypotenuse.svg/250px-Hypotenuse.svg.png`.
/// Accepts protocol-relative `//host/…` sources as found in article HTML.
pub fn media_file_name(src: &str) -> Option<String> {
    let absolute = if src.starts_with("//") {
        format!("https:{src}")
    } else {
        src.to_owned()
    };
    let url = url::Url::parse(&absolute).ok()?;
    if url.scheme() != "https" || !url.host_str().is_some_and(|h| MEDIA_HOSTS.contains(&h)) {
        return None;
    }
    let mut segments = url.path_segments()?;
    if segments.next()? != "wikipedia" {
        return None;
    }
    segments.next()?; // wiki, e.g. commons or en
    let mut rest: Vec<&str> = segments.collect();
    if rest.first() == Some(&"thumb") {
        rest.remove(0);
    }
    // Two hash directories, then the file name.
    let (a, b, name) = (rest.first()?, rest.get(1)?, rest.get(2)?);
    let hashed = a.len() == 1 && b.len() == 2 && b.starts_with(a);
    let name = percent_decode(name)?;
    (hashed && !name.is_empty() && !name.contains('/')).then_some(name)
}

/// `File:Some name.svg` → `Some_name.svg`, to compare with [`media_file_name`].
pub fn file_title_key(file_title: &str) -> String {
    file_title
        .trim_start_matches("File:")
        .trim()
        .replace(' ', "_")
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
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
    fn real_svg_files_download_as_originals_with_a_rendering_to_fall_back_to() {
        let downloads = parse_image_downloads(HYPOTENUSE_IMAGES, &files()).unwrap();
        assert_eq!(downloads.len(), 3);
        assert_eq!(
            downloads[0].url,
            "https://upload.wikimedia.org/wikipedia/commons/8/8a/Hypotenuse.svg"
        );
        assert_eq!(
            downloads[0].expected,
            Some(Original {
                mime: "image/svg+xml",
                sha1: "d9045306eb0ba275ad86d5ba8a238cca61a3c6dd".into(),
                size: 3328,
            })
        );
        assert_eq!(
            downloads[0].fallback.as_deref(),
            Some("https://thumb.wikimedia.org/wikipedia/commons/thumb/8/8a/Hypotenuse.svg/1280px-Hypotenuse.svg.png")
        );
        for (download, file) in downloads.iter().zip(files()) {
            assert_eq!(download.file_title, file);
            assert!(download.is_svg_original());
            assert_eq!(download.accept(), "image/svg+xml");
            let rendering = download.fallback_download().unwrap();
            assert_eq!(rendering.expected, None, "a rendering has no stated hash");
            assert_eq!(rendering.accept(), RENDERING_ACCEPT);
            for url in [&download.url, &rendering.url] {
                assert!(media_url_allowed(url));
                assert_eq!(
                    media_file_name(url),
                    Some(file_title_key(&file)),
                    "both map back to the file"
                );
            }
        }
        // The committed original matches Wikimedia's stated hash; a changed
        // byte does not.
        let original = include_bytes!("../../../fixtures/wikimedia/commonswiki/Hypotenuse.svg");
        assert_eq!(verify_image(&downloads[0], original), Ok("image/svg+xml"));
        let mut changed = original.to_vec();
        changed[100] ^= 1;
        assert_eq!(
            verify_image(&downloads[0], &changed),
            Err(ImageDownloadError::DigestMismatch)
        );
        // An SVG too large to keep as a vector goes straight to its rendering.
        let large = HYPOTENUSE_IMAGES.replace("\"size\":3328", "\"size\":4194305");
        let downloads = parse_image_downloads(&large, &files()).unwrap();
        assert_eq!(downloads[0].expected, None);
        assert!(downloads[0].url.contains("1280px-Hypotenuse.svg.png"));
    }

    fn reply(info: &str) -> String {
        format!(
            "{{\"batchcomplete\":true,\"query\":{{\"pages\":[{{\"ns\":6,\"title\":\"File:A.png\",\"imageinfo\":[{info}]}}]}}}}"
        )
    }

    fn one(info: &str) -> Result<ImageDownload, ImageDownloadError> {
        parse_image_downloads(&reply(info), &["File:A.png".to_owned()]).map(|mut d| d.remove(0))
    }

    #[test]
    fn small_rasters_download_as_the_original_with_its_hash_and_large_ones_as_a_rendering() {
        let small = one(
            r#"{"mime":"image/png","width":400,"size":5,"sha1":"ABCDEFABCDEFABCDEFABCDEFABCDEFABCDEFABCD",
               "url":"https://upload.wikimedia.org/wikipedia/commons/a/ab/A.png?utm_source=x",
               "thumburl":"https://thumb.wikimedia.org/wikipedia/commons/thumb/a/ab/A.png/1280px-A.png"}"#,
        )
        .unwrap();
        assert_eq!(
            small.url,
            "https://upload.wikimedia.org/wikipedia/commons/a/ab/A.png"
        );
        assert_eq!(
            small.expected,
            Some(Original {
                mime: "image/png",
                sha1: "abcdefabcdefabcdefabcdefabcdefabcdefabcd".into(),
                size: 5
            })
        );
        assert_eq!(
            small.accept(),
            "image/png",
            "an original is never converted"
        );
        let large = one(
            r#"{"mime":"image/jpeg","width":4000,"size":9000000,"sha1":"abcdefabcdefabcdefabcdefabcdefabcdefabcd",
               "url":"https://upload.wikimedia.org/wikipedia/commons/a/ab/A.jpg",
               "thumburl":"https://thumb.wikimedia.org/wikipedia/commons/thumb/a/ab/A.jpg/1280px-A.jpg"}"#,
        )
        .unwrap();
        assert_eq!(large.expected, None);
        assert!(large.url.contains("/thumb/"));
    }

    #[test]
    fn untrusted_hosts_and_unknown_formats_are_refused() {
        for info in [
            r#"{"mime":"image/svg+xml","width":10,"thumburl":"https://evil.example/wikipedia/commons/thumb/a/ab/A.svg/1280px-A.svg.png"}"#,
            r#"{"mime":"image/svg+xml","width":10,"thumburl":"http://thumb.wikimedia.org/wikipedia/commons/thumb/a/ab/A.svg/1280px-A.svg.png"}"#,
            r#"{"mime":"image/svg+xml","width":10,"thumburl":"https://thumb.wikimedia.org/w/index.php"}"#,
            r#"{"mime":"image/svg+xml","width":10,"thumburl":"https://thumb.wikimedia.org/wikipedia/commons/thumb/a/ab/A.svg/1280px-A.svg"}"#,
            r#"{"mime":"image/svg+xml","width":10}"#,
            r#"{"mime":"image/png","width":10,"size":5,"url":"https://upload.wikimedia.org/wikipedia/commons/a/ab/A.png"}"#,
        ] {
            assert!(
                matches!(one(info), Err(ImageDownloadError::NoUsableUrl(_))),
                "{info}"
            );
        }
        assert!(matches!(
            parse_image_downloads(&reply("{}"), &["File:B.png".to_owned()]),
            Err(ImageDownloadError::Metadata(
                MediaMetadataError::MissingFile(_)
            ))
        ));
    }

    #[test]
    fn downloads_must_match_their_format_size_and_hash() {
        let png = b"\x89PNG\r\n\x1a\nrest".to_vec();
        let original = ImageDownload {
            file_title: "File:A.png".into(),
            url: "https://upload.wikimedia.org/wikipedia/commons/a/ab/A.png".into(),
            expected: Some(Original {
                mime: "image/png",
                sha1: hex(&Sha1::digest(&png)),
                size: png.len() as u64,
            }),
            fallback: None,
        };
        assert_eq!(verify_image(&original, &png), Ok("image/png"));
        let mut changed = png.clone();
        *changed.last_mut().unwrap() ^= 1;
        assert_eq!(
            verify_image(&original, &changed),
            Err(ImageDownloadError::DigestMismatch)
        );
        assert_eq!(
            verify_image(&original, b"<svg onload=alert(1)>"),
            Err(ImageDownloadError::WrongFormat)
        );
        let webp = b"RIFF\x10\0\0\0WEBPVP8 ".to_vec();
        assert_eq!(
            verify_image(&original, &webp),
            Err(ImageDownloadError::WrongFormat),
            "an original converted to another format is refused"
        );
        // A rendering is stored in whichever pack format arrived.
        let rendering = ImageDownload {
            expected: None,
            ..original
        };
        assert_eq!(verify_image(&rendering, &webp), Ok("image/webp"));
        assert_eq!(verify_image(&rendering, &png), Ok("image/png"));
        assert_eq!(
            verify_image(&rendering, &[0xFF, 0xD8, 0xFF, 0xE0]),
            Ok("image/jpeg")
        );
        for refused in [&b"<svg/>"[..], b"<!doctype html>", b"%PDF-1.7", b""] {
            assert_eq!(
                verify_image(&rendering, refused),
                Err(ImageDownloadError::WrongFormat)
            );
        }
        assert_eq!(
            verify_image(&rendering, &vec![0xFF; MAX_IMAGE_BYTES + 1]),
            Err(ImageDownloadError::TooLarge)
        );
    }

    #[test]
    fn article_image_sources_map_to_their_file() {
        assert_eq!(
            media_file_name("//upload.wikimedia.org/wikipedia/commons/thumb/d/d2/Pythagorean.svg/250px-Pythagorean.svg.png"),
            Some("Pythagorean.svg".into())
        );
        assert_eq!(
            media_file_name("https://upload.wikimedia.org/wikipedia/en/e/ed/Caf%C3%A9_(1).jpg"),
            Some("Café_(1).jpg".into())
        );
        assert_eq!(file_title_key("File:Café (1).jpg"), "Café_(1).jpg");
        for src in [
            "//evil.example/wikipedia/commons/a/ab/A.png",
            "http://upload.wikimedia.org/wikipedia/commons/a/ab/A.png",
            "//upload.wikimedia.org/math/render/svg/abc",
            "//upload.wikimedia.org/wikipedia/commons/a/xy/A.png",
            "./A.png",
        ] {
            assert_eq!(media_file_name(src), None, "{src}");
        }
    }

    #[test]
    fn media_urls_are_canonical_only() {
        assert!(media_url_allowed(
            "https://upload.wikimedia.org/wikipedia/commons/a/ab/A.png"
        ));
        for url in [
            "https://upload.wikimedia.org/wikipedia/commons/a/ab/A.png?x=1",
            "https://upload.wikimedia.org:444/wikipedia/commons/a/ab/A.png",
            "https://user@upload.wikimedia.org/wikipedia/commons/a/ab/A.png",
            "https://upload.wikimedia.org/w/api.php",
            "https://en.wikipedia.org/wikipedia/commons/a/ab/A.png",
            "https://upload.wikimedia.org/wikipedia/commons/a/ab/A\".png",
        ] {
            assert!(!media_url_allowed(url), "{url}");
        }
    }
}
