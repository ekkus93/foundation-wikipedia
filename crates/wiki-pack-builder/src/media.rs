//! Validate a pack's declared offline media inventory before it can be called
//! visually complete. This does not discover HTML dependencies or hash bytes:
//! callers must derive the *complete* resource list from canonical rendering
//! and populate available_verified only after verifying object digests.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use wiki_pack_format::manifest::Manifest;
use wiki_pack_format::objects::{verify_manifest_directory, ObjectVerifyError};

const MAX_MEDIA_OBJECT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DECLARED_MEDIA_BYTES: u64 = 1_u64 << 40;
const MAX_RESOURCES: usize = 100_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceKind {
    Image,
    Diagram,
    Plot,
    Map,
    Math,
    Style,
    Font,
    AudioVideoPreview,
    AudioVideoStream,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resource {
    pub id: String,
    pub kind: ResourceKind,
    pub sha256: Option<String>,
    pub bytes: Option<u64>,
    pub preview_id: Option<String>,
    pub creator: String,
    pub license: String,
    pub attribution: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaError {
    DuplicateId(String),
    MissingMetadata(String),
    InvalidDigest(String),
    MissingVerifiedObject(String),
    ByteLengthMismatch(String),
    MissingPreview(String),
    InvalidStream(String),
    ResourceBudget(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaCompleteness {
    pub required_objects: usize,
    pub unique_verified_bytes: u64,
    pub on_demand_streams: usize,
}

fn digest_valid(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|c| c.is_ascii_hexdigit())
}

/// Verify that every declared *mandatory* offline visual is present in the
/// caller's digest-verified object set. Audio/video bytes are never required;
/// their thumbnails/previews and attribution MUST be available locally.
pub fn check_media_completeness(
    resources: &[Resource],
    available_verified: &BTreeMap<String, u64>,
) -> Result<MediaCompleteness, MediaError> {
    if resources.len() > MAX_RESOURCES {
        return Err(MediaError::ResourceBudget("resource-count".into()));
    }
    let mut by_id = BTreeMap::new();
    for resource in resources {
        if resource.id.trim().is_empty()
            || resource.creator.trim().is_empty()
            || resource.license.trim().is_empty()
            || resource.attribution.trim().is_empty()
        {
            return Err(MediaError::MissingMetadata(resource.id.clone()));
        }
        if by_id.insert(resource.id.as_str(), resource).is_some() {
            return Err(MediaError::DuplicateId(resource.id.clone()));
        }
    }

    let mut streams = 0;
    let mut counted = BTreeSet::new();
    let mut total_bytes = 0_u64;

    for resource in resources {
        if resource.kind == ResourceKind::AudioVideoStream {
            streams += 1;
            if resource.sha256.is_some() || resource.bytes.is_some() {
                return Err(MediaError::InvalidStream(resource.id.clone()));
            }
            let preview_id = resource
                .preview_id
                .as_deref()
                .ok_or_else(|| MediaError::MissingPreview(resource.id.clone()))?;
            if by_id.get(preview_id).map(|item| item.kind) != Some(ResourceKind::AudioVideoPreview)
            {
                return Err(MediaError::MissingPreview(resource.id.clone()));
            }
            continue;
        }
        if resource.preview_id.is_some() {
            return Err(MediaError::InvalidStream(resource.id.clone()));
        }
        let hash = resource
            .sha256
            .as_deref()
            .ok_or_else(|| MediaError::InvalidDigest(resource.id.clone()))?;
        if !digest_valid(hash) {
            return Err(MediaError::InvalidDigest(resource.id.clone()));
        }
        let size = resource
            .bytes
            .filter(|value| *value > 0)
            .ok_or_else(|| MediaError::MissingMetadata(resource.id.clone()))?;
        // Content-addressed object keys are canonical lowercase SHA-256.
        // Normalize the declared digest before lookup and deduplication so
        // case variants cannot inflate installed-byte accounting.
        let canonical_hash = hash.to_ascii_lowercase();
        if size > MAX_MEDIA_OBJECT_BYTES {
            return Err(MediaError::ResourceBudget(resource.id.clone()));
        }
        let observed = available_verified
            .get(&canonical_hash)
            .ok_or_else(|| MediaError::MissingVerifiedObject(resource.id.clone()))?;
        if *observed != size {
            return Err(MediaError::ByteLengthMismatch(resource.id.clone()));
        }
        if counted.insert(canonical_hash) {
            total_bytes = total_bytes
                .checked_add(size)
                .filter(|total| *total <= MAX_DECLARED_MEDIA_BYTES)
                .ok_or_else(|| MediaError::ResourceBudget(resource.id.clone()))?;
        }
    }

    Ok(MediaCompleteness {
        required_objects: counted.len(),
        unique_verified_bytes: total_bytes,
        on_demand_streams: streams,
    })
}

/// A verified-staging guard for *declared* required media. Successful results
/// cannot certify a complete offline pack unless a separate trusted renderer
/// has also discovered every HTML, CSS, font and math dependency.
#[derive(Debug)]
pub enum StagedMediaError {
    Objects(ObjectVerifyError),
    Media(MediaError),
    ConflictingDigest(String),
}

/// Rehash every object from a validated provisional pack manifest, then
/// require every declared offline visual and A/V preview to be among those
/// verified bytes. Audio/video stream payloads are never bundled by default.
///
/// IMPORTANT: The manifest and resource list are caller-supplied. Their
/// authenticity, semantic mapping to rendered article content and licensing
/// are not established by this function. The caller must own an immutable
/// staging tree throughout both passes.
pub fn check_declared_staged_media(
    manifest: &Manifest,
    root: &Path,
    resources: &[Resource],
) -> Result<MediaCompleteness, StagedMediaError> {
    verify_manifest_directory(manifest, root).map_err(StagedMediaError::Objects)?;
    let mut verified = BTreeMap::new();
    for object in &manifest.objects {
        // A verified article, index or arbitrary object is not an installable
        // media asset, even when its bytes happen to have the requested hash.
        // Keep this in sync with wiki-pack-format::media_coverage.
        if !object.path.starts_with("media/") {
            continue;
        }
        let hash = object.sha256.to_ascii_lowercase();
        match verified.insert(hash.clone(), object.bytes) {
            Some(previous) if previous != object.bytes => {
                return Err(StagedMediaError::ConflictingDigest(hash));
            }
            _ => {}
        }
    }
    check_media_completeness(resources, &verified).map_err(StagedMediaError::Media)
}

#[cfg(test)]
mod staged_tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};
    use wiki_pack_format::export::custom_manifest_from_tree;

    fn media(id: &str, kind: ResourceKind, digest: Option<String>, bytes: Option<u64>) -> Resource {
        Resource {
            id: id.into(),
            kind,
            sha256: digest,
            bytes,
            preview_id: None,
            creator: "Contributor".into(),
            license: "CC BY-SA 4.0".into(),
            attribution: "Diagram by Contributor".into(),
        }
    }

    #[test]
    fn non_media_manifest_object_cannot_satisfy_visual_requirement() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-media-namespace-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("articles")).unwrap();
        let bytes = b"same bytes as expected diagram";
        fs::write(root.join("articles/page.cbor"), bytes).unwrap();
        let manifest =
            custom_manifest_from_tree(&root, "physics", "enwiki", "20261001", "physics").unwrap();
        let digest = manifest.objects[0].sha256.clone();
        let expected = media(
            "diagram",
            ResourceKind::Diagram,
            Some(digest),
            Some(bytes.len() as u64),
        );
        assert!(matches!(
            check_declared_staged_media(&manifest, &root, &[expected.clone()]),
            Err(StagedMediaError::Media(MediaError::MissingVerifiedObject(_)))
        ));

        fs::create_dir(root.join("media")).unwrap();
        fs::write(root.join("media/diagram.svg"), bytes).unwrap();
        let with_media =
            custom_manifest_from_tree(&root, "physics", "enwiki", "20261001", "physics").unwrap();
        let result = check_declared_staged_media(&with_media, &root, &[expected]).unwrap();
        assert_eq!(result.required_objects, 1);
        assert_eq!(result.unique_verified_bytes, bytes.len() as u64);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn staged_pack_requires_verified_visuals_and_offline_av_previews() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("wiki-media-gate-{}-{stamp}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("media")).unwrap();
        let diagram = b"complete visual";
        let preview = b"audio/video offline preview";
        fs::write(root.join("media/plot.svg"), diagram).unwrap();
        fs::write(root.join("media/preview.jpg"), preview).unwrap();
        let manifest =
            custom_manifest_from_tree(&root, "physics", "enwiki", "20261001", "physics").unwrap();
        let plot_hash = manifest
            .objects
            .iter()
            .find(|item| item.path == "media/plot.svg")
            .unwrap()
            .sha256
            .clone();
        let preview_hash = manifest
            .objects
            .iter()
            .find(|item| item.path == "media/preview.jpg")
            .unwrap()
            .sha256
            .clone();
        let mut resources = vec![
            media(
                "plot",
                ResourceKind::Plot,
                Some(plot_hash),
                Some(diagram.len() as u64),
            ),
            media(
                "preview",
                ResourceKind::AudioVideoPreview,
                Some(preview_hash),
                Some(preview.len() as u64),
            ),
        ];
        let mut stream = media("lecture", ResourceKind::AudioVideoStream, None, None);
        stream.preview_id = Some("preview".into());
        resources.push(stream);
        let report = check_declared_staged_media(&manifest, &root, &resources).unwrap();
        assert_eq!(report.required_objects, 2);
        assert_eq!(report.on_demand_streams, 1);
        assert_eq!(
            report.unique_verified_bytes,
            (diagram.len() + preview.len()) as u64
        );
        let mut without_preview = resources.clone();
        without_preview.remove(1);
        assert!(matches!(
            check_declared_staged_media(&manifest, &root, &without_preview),
            Err(StagedMediaError::Media(MediaError::MissingPreview(_)))
        ));
        let mut absent = resources.clone();
        absent[0].sha256 = Some("0".repeat(64));
        assert!(matches!(
            check_declared_staged_media(&manifest, &root, &absent),
            Err(StagedMediaError::Media(MediaError::MissingVerifiedObject(
                _
            )))
        ));
        fs::write(root.join("media/plot.svg"), b"complete visuAl").unwrap();
        assert!(matches!(
            check_declared_staged_media(&manifest, &root, &resources),
            Err(StagedMediaError::Objects(ObjectVerifyError::HashMismatch(
                _
            )))
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
