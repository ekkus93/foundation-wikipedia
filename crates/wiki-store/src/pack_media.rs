//! PACK-003: the images an offline pack carries.
//!
//! A pack with images holds each distinct image once, as the object
//! `media/<sha256>`, and lists them in the canonical index
//! [`MEDIA_INDEX_OBJECT`] (the manifest's attribution object). Licence,
//! creator and attribution stay with each article's `MediaAsset`, so the same
//! image used under two notices keeps both.
//!
//! Installing checks that the pack is complete: every image an article
//! requires is present with the declared format, and nothing else is.
//! A pack without the index is text only, and none of its articles may
//! claim installed image bytes. Reading re-hashes the bytes every time.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use sha2::{Digest, Sha256};
use wiki_model::Article;
use wiki_pack_format::manifest::Object;

/// Directory of image objects inside a pack.
pub const MEDIA_DIR: &str = "media";
/// The canonical image index, declared as the manifest attribution object.
pub const MEDIA_INDEX_OBJECT: &str = "media/index.json";
/// Image formats a pack may carry: browsers decode them without scripts.
pub const PACK_IMAGE_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/webp"];
const MAX_INDEX_BYTES: u64 = 16 * 1024 * 1024;
/// Largest single image object.
pub const MAX_PACK_IMAGE_BYTES: u64 = 8 * 1024 * 1024;

/// One stored image.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PackImage {
    pub sha256: String,
    pub mime: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackMediaError {
    /// The index is missing, malformed or not in canonical form.
    InvalidIndex,
    /// The media objects do not match the index exactly.
    ObjectMismatch(String),
    /// An article requires an image the pack does not hold (or holds in
    /// another format).
    MissingImage(String),
    /// The pack holds an image no article uses.
    UnusedImage(String),
    /// A pack with images has an article image without bytes.
    IncompleteArticle(String),
    /// A text-only pack claims image bytes or carries media objects.
    UndeclaredMedia,
    /// Declared media count differs from the index.
    CountMismatch,
    /// Stored bytes differ from their name.
    Corrupt,
    Io(String),
}

/// Whether a pack object is an image or the image index: never an article
/// shard. The installer and every reopening of a snapshot must agree.
pub fn is_media_object(path: &str) -> bool {
    path.starts_with("media/")
}

pub fn media_object_path(sha256: &str) -> String {
    format!("{MEDIA_DIR}/{sha256}")
}

fn sha256_name(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn valid_image(image: &PackImage) -> bool {
    sha256_name(&image.sha256)
        && PACK_IMAGE_TYPES.contains(&image.mime.as_str())
        && image.bytes > 0
        && image.bytes <= MAX_PACK_IMAGE_BYTES
}

/// Canonical index bytes: one entry per digest, sorted by digest.
pub fn encode_media_index(images: &[PackImage]) -> Result<Vec<u8>, PackMediaError> {
    let mut sorted = images.to_vec();
    sorted.sort();
    sorted.dedup();
    if sorted.windows(2).any(|w| w[0].sha256 == w[1].sha256) || !sorted.iter().all(valid_image) {
        return Err(PackMediaError::InvalidIndex);
    }
    let mut out = String::from("{\"version\":1,\"images\":[");
    for (i, image) in sorted.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"sha256\":\"{}\",\"mime\":\"{}\",\"bytes\":{}}}",
            image.sha256, image.mime, image.bytes
        ));
    }
    out.push_str("]}\n");
    Ok(out.into_bytes())
}

/// Parse an index; only the exact canonical encoding is accepted.
pub fn parse_media_index(bytes: &[u8]) -> Result<Vec<PackImage>, PackMediaError> {
    let invalid = || PackMediaError::InvalidIndex;
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    if value.get("version").and_then(serde_json::Value::as_u64) != Some(1) {
        return Err(invalid());
    }
    let images = value
        .get("images")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(invalid)?
        .iter()
        .map(|entry| {
            Some(PackImage {
                sha256: entry.get("sha256")?.as_str()?.to_owned(),
                mime: entry.get("mime")?.as_str()?.to_owned(),
                bytes: entry.get("bytes")?.as_u64()?,
            })
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(invalid)?;
    if encode_media_index(&images)? != bytes {
        return Err(invalid());
    }
    Ok(images)
}

/// The images `articles` require, by digest, with the stated format. Fails
/// if one digest is claimed with two formats.
pub fn required_images<'a>(
    articles: impl IntoIterator<Item = &'a Article>,
) -> Result<(BTreeMap<String, String>, Vec<String>), PackMediaError> {
    let mut required = BTreeMap::new();
    let mut without_bytes = Vec::new();
    for article in articles {
        for asset in &article.media {
            match &asset.sha256 {
                Some(digest) => {
                    if let Some(previous) = required.insert(digest.clone(), asset.mime_type.clone())
                    {
                        if previous != asset.mime_type {
                            return Err(PackMediaError::MissingImage(digest.clone()));
                        }
                    }
                }
                None => without_bytes.push(article.title.clone()),
            }
        }
    }
    Ok((required, without_bytes))
}

/// Check a staged pack's images against its manifest and its articles.
///
/// `objects` are the manifest objects (whose bytes staging already
/// verified), `index_object` the declared attribution object, `required`
/// and `without_bytes` come from [`required_images`] over every article.
pub fn check_pack_media(
    staged: &Path,
    objects: &[Object],
    index_object: Option<&str>,
    declared_count: u64,
    required: &BTreeMap<String, String>,
    without_bytes: &[String],
) -> Result<(), PackMediaError> {
    let media_objects: BTreeMap<&str, &Object> = objects
        .iter()
        .filter(|object| is_media_object(&object.path) && object.path != MEDIA_INDEX_OBJECT)
        .map(|object| (object.path.as_str(), object))
        .collect();
    if index_object != Some(MEDIA_INDEX_OBJECT) {
        // Text only (any other attribution object holds text notices):
        // images stay online, and nothing may claim otherwise.
        if !media_objects.is_empty() || !required.is_empty() || declared_count != 0 {
            return Err(PackMediaError::UndeclaredMedia);
        }
        return Ok(());
    }
    if let Some(title) = without_bytes.first() {
        return Err(PackMediaError::IncompleteArticle(title.clone()));
    }
    let images = parse_media_index(&read_bounded(
        &staged.join(MEDIA_INDEX_OBJECT),
        MAX_INDEX_BYTES,
    )?)?;
    if images.len() as u64 != declared_count {
        return Err(PackMediaError::CountMismatch);
    }
    let mut listed = BTreeSet::new();
    for image in &images {
        let path = media_object_path(&image.sha256);
        let object = media_objects
            .get(path.as_str())
            .ok_or_else(|| PackMediaError::ObjectMismatch(path.clone()))?;
        if object.sha256.to_ascii_lowercase() != image.sha256 || object.bytes != image.bytes {
            return Err(PackMediaError::ObjectMismatch(path));
        }
        listed.insert(path);
        match required.get(&image.sha256) {
            Some(mime) if *mime == image.mime => {}
            Some(_) => return Err(PackMediaError::MissingImage(image.sha256.clone())),
            None => return Err(PackMediaError::UnusedImage(image.sha256.clone())),
        }
    }
    if let Some(extra) = media_objects.keys().find(|path| !listed.contains(**path)) {
        return Err(PackMediaError::ObjectMismatch((*extra).to_owned()));
    }
    if let Some(missing) = required
        .keys()
        .find(|digest| !images.iter().any(|image| &&image.sha256 == digest))
    {
        return Err(PackMediaError::MissingImage(missing.clone()));
    }
    Ok(())
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, PackMediaError> {
    let io = |error: io::Error| PackMediaError::Io(error.to_string());
    let file = File::open(path).map_err(io)?;
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes).map_err(io)?;
    if bytes.len() as u64 > limit {
        return Err(PackMediaError::InvalidIndex);
    }
    Ok(bytes)
}

/// Read one image of an installed snapshot: `None` if the snapshot holds no
/// such image. The bytes are re-hashed against their name on every read.
pub fn read_pack_image(
    snapshot: &Path,
    sha256: &str,
) -> Result<Option<(String, Vec<u8>)>, PackMediaError> {
    if !sha256_name(sha256) {
        return Ok(None);
    }
    let index_path = snapshot.join(MEDIA_INDEX_OBJECT);
    if !index_path.is_file() {
        return Ok(None);
    }
    let images = parse_media_index(&read_bounded(&index_path, MAX_INDEX_BYTES)?)?;
    let Some(image) = images.into_iter().find(|image| image.sha256 == sha256) else {
        return Ok(None);
    };
    let bytes = read_bounded(
        &snapshot.join(media_object_path(sha256)),
        MAX_PACK_IMAGE_BYTES,
    )?;
    let actual = format!("{:x}", Sha256::digest(&bytes));
    if actual != sha256 || bytes.len() as u64 != image.bytes {
        return Err(PackMediaError::Corrupt);
    }
    Ok(Some((image.mime, bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(seed: u8, mime: &str) -> (PackImage, Vec<u8>) {
        let bytes = vec![seed; 10];
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        (
            PackImage {
                sha256,
                mime: mime.into(),
                bytes: 10,
            },
            bytes,
        )
    }

    #[test]
    fn the_index_has_one_canonical_form() {
        let (a, _) = image(1, "image/png");
        let (b, _) = image(2, "image/jpeg");
        let encoded = encode_media_index(&[b.clone(), a.clone(), a.clone()]).unwrap();
        assert_eq!(parse_media_index(&encoded).unwrap().len(), 2);
        let spaced = String::from_utf8(encoded.clone())
            .unwrap()
            .replace(",\"mime\"", ", \"mime\"");
        assert_eq!(
            parse_media_index(spaced.as_bytes()),
            Err(PackMediaError::InvalidIndex)
        );
        let svg = PackImage {
            mime: "image/svg+xml".into(),
            ..a.clone()
        };
        assert_eq!(
            encode_media_index(&[svg]),
            Err(PackMediaError::InvalidIndex),
            "no SVG markup in packs"
        );
        let conflicting = PackImage {
            mime: "image/gif".into(),
            ..a
        };
        assert!(encode_media_index(&[conflicting, image(1, "image/png").0]).is_err());
    }

    fn object(image: &PackImage) -> Object {
        Object {
            path: media_object_path(&image.sha256),
            sha256: image.sha256.clone(),
            bytes: image.bytes,
        }
    }

    #[test]
    fn a_pack_must_hold_exactly_the_images_its_articles_require() {
        let dir =
            std::env::temp_dir().join(format!("pack-media-{}-{}", std::process::id(), line!()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(MEDIA_DIR)).unwrap();
        let (a, a_bytes) = image(1, "image/png");
        let (b, _) = image(2, "image/jpeg");
        std::fs::write(
            dir.join(MEDIA_INDEX_OBJECT),
            encode_media_index(&[a.clone()]).unwrap(),
        )
        .unwrap();
        std::fs::write(dir.join(media_object_path(&a.sha256)), &a_bytes).unwrap();
        let objects = vec![object(&a)];
        let needs = |list: &[&PackImage]| {
            list.iter()
                .map(|i| (i.sha256.clone(), i.mime.clone()))
                .collect::<BTreeMap<_, _>>()
        };
        let check = |objects: &[Object], count, required: &BTreeMap<_, _>, without: &[String]| {
            check_pack_media(
                &dir,
                objects,
                Some(MEDIA_INDEX_OBJECT),
                count,
                required,
                without,
            )
        };
        assert_eq!(check(&objects, 1, &needs(&[&a]), &[]), Ok(()));
        assert_eq!(
            check(&objects, 1, &needs(&[&a, &b]), &[]),
            Err(PackMediaError::MissingImage(b.sha256.clone()))
        );
        assert_eq!(
            check(&objects, 1, &BTreeMap::new(), &[]),
            Err(PackMediaError::UnusedImage(a.sha256.clone()))
        );
        let as_jpeg = BTreeMap::from([(a.sha256.clone(), "image/jpeg".to_owned())]);
        assert!(matches!(
            check(&objects, 1, &as_jpeg, &[]),
            Err(PackMediaError::MissingImage(_))
        ));
        assert_eq!(
            check(&objects, 1, &needs(&[&a]), &["Hypotenuse".into()]),
            Err(PackMediaError::IncompleteArticle("Hypotenuse".into()))
        );
        assert_eq!(
            check(&objects, 2, &needs(&[&a]), &[]),
            Err(PackMediaError::CountMismatch)
        );
        let extra = vec![object(&a), object(&b)];
        assert!(matches!(
            check(&extra, 1, &needs(&[&a]), &[]),
            Err(PackMediaError::ObjectMismatch(_))
        ));
        // Text-only packs may not carry or claim images.
        assert_eq!(
            check_pack_media(&dir, &objects, None, 0, &BTreeMap::new(), &[]),
            Err(PackMediaError::UndeclaredMedia)
        );
        assert_eq!(
            check_pack_media(&dir, &[], None, 0, &needs(&[&a]), &[]),
            Err(PackMediaError::UndeclaredMedia)
        );
        assert_eq!(
            check_pack_media(&dir, &[], None, 0, &BTreeMap::new(), &["T".into()]),
            Ok(())
        );

        // Reading re-hashes.
        let (mime, bytes) = read_pack_image(&dir, &a.sha256).unwrap().unwrap();
        assert_eq!((mime.as_str(), bytes), ("image/png", a_bytes));
        assert_eq!(read_pack_image(&dir, &b.sha256), Ok(None));
        assert_eq!(read_pack_image(&dir, "../index.json"), Ok(None));
        std::fs::write(dir.join(media_object_path(&a.sha256)), vec![9u8; 10]).unwrap();
        assert_eq!(
            read_pack_image(&dir, &a.sha256),
            Err(PackMediaError::Corrupt)
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
