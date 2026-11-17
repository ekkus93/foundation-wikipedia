//! PACK-003: which bytes to download for each image an article requires,
//! and how to check them before they go into a pack.
//!
//! The `imageinfo` reply (requested with `size|sha1` and
//! [`OFFLINE_IMAGE_WIDTH`]) is untrusted. For each file:
//!
//! * a PNG, JPEG, GIF or WebP no wider than [`OFFLINE_IMAGE_WIDTH`] is
//!   downloaded as the original, and must match Wikimedia's stated size and
//!   SHA-1;
//! * anything else — SVG, a wider raster, TIFF, PDF, a video still — is
//!   downloaded as Wikimedia's own rendering at that width. Wikimedia states
//!   no hash for a rendering, so only TLS to the fixed media hosts, the size
//!   cap and the file signature check it. SVG is rasterised rather than
//!   sanitised, so no SVG markup is ever displayed from a pack.
//!
//! Every download must start with the signature of the format it claims, so
//! a mislabelled or substituted reply is refused.

use serde_json::Value;
use sha1::{Digest, Sha1};

use crate::online_requests::OFFLINE_IMAGE_WIDTH;
use crate::rendered_media::{image_infos, MediaMetadataError};

/// Largest image accepted into a pack.
pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;

/// Hosts Wikimedia serves file bytes from.
const MEDIA_HOSTS: [&str; 2] = ["upload.wikimedia.org", "thumb.wikimedia.org"];

/// Fetches image bytes. Implementations must accept only URLs for which
/// [`media_url_allowed`] holds, never follow redirects and keep TLS checks.
pub trait MediaFileTransport {
    type Error;
    fn get_media(&mut self, url: &str, max_bytes: usize) -> Result<Vec<u8>, Self::Error>;
}

/// One image to download for a pack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageDownload {
    /// `File:` title as requested.
    pub file_title: String,
    /// Canonical media URL (no query or fragment).
    pub url: String,
    /// Format of the bytes that will be downloaded.
    pub mime: &'static str,
    /// Wikimedia's SHA-1 and size, for an original download.
    pub expected: Option<(String, u64)>,
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
                mime,
                expected: Some((sha1, size)),
            });
        }
    }
    let url = info
        .get("thumburl")
        .and_then(Value::as_str)
        .and_then(canonical_media_url)
        .ok_or_else(unusable)?;
    let mime = rendering_mime(&url).ok_or_else(unusable)?;
    Ok(ImageDownload {
        file_title: file.to_owned(),
        url,
        mime,
        expected: None,
    })
}

fn signature_matches(mime: &str, bytes: &[u8]) -> bool {
    match mime {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "image/webp" => bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        _ => false,
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// Check downloaded bytes against what was asked for.
pub fn verify_image(download: &ImageDownload, bytes: &[u8]) -> Result<(), ImageDownloadError> {
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(ImageDownloadError::TooLarge);
    }
    if !signature_matches(download.mime, bytes) {
        return Err(ImageDownloadError::WrongFormat);
    }
    if let Some((sha1, size)) = &download.expected {
        let actual = hex(&Sha1::digest(bytes));
        if bytes.len() as u64 != *size || &actual != sha1 {
            return Err(ImageDownloadError::DigestMismatch);
        }
    }
    Ok(())
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
    fn real_svg_files_download_as_wikimedia_png_renderings() {
        let downloads = parse_image_downloads(HYPOTENUSE_IMAGES, &files()).unwrap();
        assert_eq!(downloads.len(), 3);
        assert_eq!(
            downloads[0].url,
            "https://thumb.wikimedia.org/wikipedia/commons/thumb/8/8a/Hypotenuse.svg/1280px-Hypotenuse.svg.png"
        );
        for (download, file) in downloads.iter().zip(files()) {
            assert_eq!(download.file_title, file);
            assert_eq!(download.mime, "image/png");
            assert_eq!(download.expected, None, "a rendering has no stated hash");
            assert!(media_url_allowed(&download.url));
            assert_eq!(
                media_file_name(&download.url),
                Some(file_title_key(&file)),
                "the rendering maps back to its file"
            );
        }
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
            Some(("abcdefabcdefabcdefabcdefabcdefabcdefabcd".into(), 5))
        );
        let large = one(
            r#"{"mime":"image/jpeg","width":4000,"size":9000000,"sha1":"abcdefabcdefabcdefabcdefabcdefabcdefabcd",
               "url":"https://upload.wikimedia.org/wikipedia/commons/a/ab/A.jpg",
               "thumburl":"https://thumb.wikimedia.org/wikipedia/commons/thumb/a/ab/A.jpg/1280px-A.jpg"}"#,
        )
        .unwrap();
        assert_eq!(large.mime, "image/jpeg");
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
        let sha1 = hex(&Sha1::digest(&png));
        let original = ImageDownload {
            file_title: "File:A.png".into(),
            url: "https://upload.wikimedia.org/wikipedia/commons/a/ab/A.png".into(),
            mime: "image/png",
            expected: Some((sha1.clone(), png.len() as u64)),
        };
        assert_eq!(verify_image(&original, &png), Ok(()));
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
        let rendering = ImageDownload {
            expected: None,
            mime: "image/jpeg",
            ..original
        };
        assert_eq!(
            verify_image(&rendering, &png),
            Err(ImageDownloadError::WrongFormat),
            "a PNG is not the JPEG that was asked for"
        );
        assert_eq!(verify_image(&rendering, &[0xFF, 0xD8, 0xFF, 0xE0]), Ok(()));
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
